# Pico firmware (dumb client)

The family data and HTML layout live on the Rust server. This directory is
only the contract the Plus 2 W must speak.

See [`PROTOCOL.md`](PROTOCOL.md).

Until an embassy port exists, bring the Inky 13.3 up with the community C
driver ([el133-pico-driver](https://github.com/dmellok/el133-pico-driver))
and replace its image source with `GET /frame.bin?checksum=…`.
