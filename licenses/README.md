# Third-party components under other licences

| Component | Used for | Licence | Source |
|---|---|---|---|
| [occt-wasm](https://github.com/andymai/occt-wasm) 5.17.1: Open CASCADE Technology compiled to WebAssembly | Reading STEP and BREP models | The WebAssembly file: LGPL-2.1 ([OCCT-LGPL-2.1.txt](OCCT-LGPL-2.1.txt)); its JavaScript: MIT or Apache-2.0 | [occt-wasm](https://github.com/andymai/occt-wasm), [Open CASCADE Technology](https://dev.opencascade.org/resources/download) |
| [acadrust](https://github.com/hakanaktt/acadrust) | Reading DWG and DXF drawings | MPL-2.0 | [crates.io](https://crates.io/crates/acadrust) |

Both are used unmodified.

The OpenCascade WebAssembly file is installed as a file of its own, not built into the program:
`occt/occt-wasm.wasm` in the installation folder (Windows: `%LOCALAPPDATA%\Omni Look`). Replacing it
with another build of the same version of occt-wasm (from its source, or built yourself) changes
what Omni Look uses.
