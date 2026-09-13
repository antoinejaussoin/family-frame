async function loadChecksum() {
  const el = document.getElementById("checksum");
  try {
    const res = await fetch("/frame.json");
    if (!res.ok) {
      el.textContent =
        "Raster is not ready yet (Chrome needed for /frame.bin). The HTML panel above is still the layout you edit.";
      return;
    }
    const data = await res.json();
    el.textContent =
      "Checksum " + data.checksum.slice(0, 16) + "… · " + data.bytes + " bytes · Pico sends this back to skip a matching frame.";
  } catch (err) {
    el.textContent = "Could not load checksum: " + err;
  }
}

loadChecksum();
