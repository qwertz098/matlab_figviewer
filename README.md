# matlab_figviewer

[![Build](https://github.com/qwertz098/matlab_figviewer/actions/workflows/build.yml/badge.svg)](https://github.com/qwertz098/matlab_figviewer/actions/workflows/build.yml)

**Download:** die aktuelle `figviewer.exe` gibt es unter
[Releases](https://github.com/qwertz098/matlab_figviewer/releases/latest) (keine Installation nötig).
Entwicklungsstände liegen als Artefakt `figviewer-windows` bei jedem [Actions-Lauf](https://github.com/qwertz098/matlab_figviewer/actions).

Kleines Windows-Tool (Rust, egui) zum Ansehen von MATLAB-`.fig`-Dateien (MAT v5).

- `.fig` auf die exe oder ins Fenster ziehen
- Ein Tab je Signalgruppe (Text vor dem ersten Punkt, z. B. `50Hz`, `10kHz`)
- Legende: Signale ein-/ausblenden, Faktor je Signal, Filter, Werte unter dem Mauszeiger
- Zoom mit Rechteck (linke Maustaste), Verschieben mit rechter Maustaste, Einpassen von X/Y
- Einrasten auf Datenpunkte: fette Linie plus Zeit/Wert/Signalname

Bauen: `cargo build --release` (C-Laufzeit ist statisch gelinkt, siehe `.cargo/config.toml`).
Grafik: erst wgpu (DX12/Vulkan), bei Fehlern OpenGL, sonst Fehlerfenster.

Tests: `cargo test` (Parser gegen selbst erzeugte, abgeschnittene und beschädigte MAT-Dateien).

Release: Tag `vX.Y.Z` pushen (`git tag v0.1.0 && git push origin v0.1.0`); die Pipeline baut die exe und hängt sie an ein GitHub-Release.
