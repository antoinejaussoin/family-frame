//! USB CDC serial CLI: `wifi`, `psk`, `server`, `save`, `show`, `clear`, `help`.
//!
//! Same commands as the laser-tag temperature / IR-capture nodes.

use core::fmt::Write as _;
use embassy_executor::Spawner;
use embassy_rp::Peri;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::Driver;
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::{Builder, Config, UsbDevice};
use heapless::String;
use static_cell::StaticCell;

use crate::battery;
use crate::board::Irqs;
use crate::power;
use crate::settings::{self, SharedFlash};
use crate::wifi;

type UsbDriver = Driver<'static, USB>;

pub fn start(spawner: Spawner, usb: Peri<'static, USB>, flash: &'static SharedFlash) {
    let driver = Driver::new(usb, Irqs);
    let mut config = Config::new(0xc0de, 0xcaed);
    config.manufacturer = Some("family-frame");
    config.product = Some("Pico LiPo 2 XL W");
    config.serial_number = Some("0001");
    config.max_power = 100;
    config.max_packet_size_0 = 64;

    static CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static MSOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();
    static STATE: StaticCell<State> = StaticCell::new();

    let mut builder = Builder::new(
        driver,
        config,
        CONFIG_DESC.init([0; 256]),
        BOS_DESC.init([0; 256]),
        MSOS_DESC.init([0; 256]),
        CONTROL_BUF.init([0; 64]),
    );
    let class = CdcAcmClass::new(&mut builder, STATE.init(State::new()), 64);
    let usb_dev = builder.build();

    spawner.spawn(usb_task(usb_dev).unwrap());
    spawner.spawn(cli_task(class, flash).unwrap());
}

#[embassy_executor::task]
async fn usb_task(mut usb: UsbDevice<'static, UsbDriver>) -> ! {
    usb.run().await
}

#[embassy_executor::task]
async fn cli_task(class: CdcAcmClass<'static, UsbDriver>, flash: &'static SharedFlash) -> ! {
    run_cli(class, flash).await
}

async fn run_cli(mut class: CdcAcmClass<'static, UsbDriver>, flash: &'static SharedFlash) -> ! {
    loop {
        class.wait_connection().await;
        let mut line = String::<160>::new();
        let mut buf = [0u8; 64];
        let _ = write_text(
            &mut class,
            "\r\nfamily-frame (Pico LiPo 2 XL W). Type help.\r\n> ",
        )
        .await;
        loop {
            match class.read_packet(&mut buf).await {
                Ok(n) => {
                    for &b in &buf[..n] {
                        match b {
                            b'\r' | b'\n' => {
                                let _ = class.write_packet(b"\r\n").await;
                                if !line.is_empty() {
                                    handle_line(&mut class, flash, line.as_str()).await;
                                    line.clear();
                                }
                                let _ = write_text(&mut class, "> ").await;
                            }
                            0x08 | 0x7f => {
                                if line.pop().is_some() {
                                    let _ = write_text(&mut class, "\x08 \x08").await;
                                }
                            }
                            b if b.is_ascii_graphic() || b == b' ' => {
                                if line.push(b as char).is_ok() {
                                    let _ = class.write_packet(&[b]).await;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Err(_) => break,
            }
        }
    }
}

async fn handle_line(
    class: &mut CdcAcmClass<'static, UsbDriver>,
    flash: &'static SharedFlash,
    line: &str,
) {
    let line = line.trim();
    let (cmd, rest) = split_cmd(line);
    match cmd {
        "help" | "?" => {
            let _ = write_text(
                class,
                "wifi <ssid>\r\n\
                 psk <password>   (empty = open network)\r\n\
                 server <host:port>\r\n\
                   e.g. 192.168.0.251:8765\r\n\
                 save             write flash and join Wi-Fi\r\n\
                 show\r\n\
                 forget           drop last frame checksum (force next paint)\r\n\
                 clear            erase saved settings\r\n",
            )
            .await;
        }
        "wifi" => {
            if rest.is_empty() || rest.len() > 32 {
                let _ = write_text(class, "usage: wifi <ssid> (max 32)\r\n").await;
                return;
            }
            settings::update(|cfg| {
                cfg.ssid.clear();
                let _ = cfg.ssid.push_str(rest);
            })
            .await;
            let _ = write_text(class, "ok. type save when ready.\r\n").await;
        }
        "psk" => {
            if rest.len() > 64 {
                let _ = write_text(class, "psk too long (max 64)\r\n").await;
                return;
            }
            settings::update(|cfg| {
                cfg.psk.clear();
                let _ = cfg.psk.push_str(rest);
            })
            .await;
            let _ = write_text(class, "ok. type save when ready.\r\n").await;
        }
        "server" => {
            if settings::parse_server(rest).is_none() {
                let _ = write_text(
                    class,
                    "usage: server <host:port>\r\n  example: 192.168.0.251:8765\r\n",
                )
                .await;
                return;
            }
            settings::update(|cfg| {
                cfg.server.clear();
                let _ = cfg.server.push_str(rest);
            })
            .await;
            let _ = write_text(class, "ok. type save when ready.\r\n").await;
        }
        "show" => {
            let cfg = settings::snapshot().await;
            let (mv, pct) = battery::last();
            let mut msg = String::<384>::new();
            let _ = write!(
                msg,
                "ssid: {}\r\npsk:  {}\r\nurl:  {}\r\nsleep: {} s (from server)\r\nchecksum: {}\r\n{}\r\nbattery: {} mV ~{}%\r\nleds: {}\r\nwake: {}\r\n",
                cfg.ssid,
                if cfg.psk.is_empty() {
                    "(none)"
                } else {
                    "(set)"
                },
                cfg.server,
                cfg.sleep_s,
                if cfg.last_checksum.is_empty() {
                    "(none)"
                } else {
                    cfg.last_checksum.as_str()
                },
                wifi::status_line(),
                mv,
                pct,
                if power::on_usb() {
                    "USB host (user LED on)"
                } else {
                    "battery (user LED off)"
                },
                power::wake_label()
            );
            let _ = write_text(class, msg.as_str()).await;
        }
        "save" => {
            let cfg = settings::snapshot().await;
            if !cfg.is_ready() {
                let _ = write_text(class, "need wifi and server first.\r\n").await;
                return;
            }
            let mut flash = flash.lock().await;
            match settings::save_flash(&mut flash, &cfg) {
                Ok(()) => {
                    settings::request_rejoin();
                    let _ = write_text(class, "saved. joining wifi...\r\n").await;
                }
                Err(_) => {
                    let _ = write_text(class, "flash write failed.\r\n").await;
                }
            }
        }
        "forget" => {
            settings::update(|cfg| {
                cfg.last_checksum.clear();
            })
            .await;
            let cfg = settings::snapshot().await;
            if cfg.is_ready() {
                let mut flash = flash.lock().await;
                let _ = settings::save_flash(&mut flash, &cfg);
            }
            let _ = write_text(class, "checksum cleared.\r\n").await;
        }
        "clear" => {
            settings::replace(settings::NetConfig::empty()).await;
            let mut flash = flash.lock().await;
            let _ = settings::erase_flash(&mut flash);
            settings::request_rejoin();
            let _ = write_text(class, "cleared.\r\n").await;
        }
        _ => {
            let _ = write_text(class, "unknown command. type help.\r\n").await;
        }
    }
}

fn split_cmd(line: &str) -> (&str, &str) {
    match line.split_once(char::is_whitespace) {
        Some((cmd, rest)) => (cmd, rest.trim()),
        None => (line, ""),
    }
}

async fn write_text(class: &mut CdcAcmClass<'static, UsbDriver>, text: &str) -> Result<(), ()> {
    for chunk in text.as_bytes().chunks(64) {
        class.write_packet(chunk).await.map_err(|_| ())?;
    }
    Ok(())
}
