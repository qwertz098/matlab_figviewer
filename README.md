# matlab_figviewer

Kleines Windows-Tool (Rust, egui) zum Ansehen von MATLAB-`.fig`-Dateien (MAT v5).

- `.fig` auf die exe oder ins Fenster ziehen
- Ein Tab je Signalgruppe (Text vor dem ersten Punkt, z. B. `50Hz`, `10kHz`)
- Legende: Signale ein-/ausblenden, Faktor je Signal, Filter, Werte unter dem Mauszeiger
- Zoom mit Rechteck (linke Maustaste), Verschieben mit rechter Maustaste, Einpassen von X/Y
- Einrasten auf Datenpunkte: fette Linie plus Zeit/Wert/Signalname

Bauen: `cargo build --release` (C-Laufzeit ist statisch gelinkt, siehe `.cargo/config.toml`).
Grafik: erst wgpu (DX12/Vulkan), bei Fehlern OpenGL, sonst Fehlerfenster.
