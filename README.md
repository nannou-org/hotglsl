# hotglsl [![Build Status](https://travis-ci.org/nannou-org/hotglsl.svg?branch=master)](https://travis-ci.org/nannou-org/hotglsl) [![Crates.io](https://img.shields.io/crates/v/hotglsl.svg)](https://crates.io/crates/hotglsl) [![Crates.io](https://img.shields.io/crates/l/hotglsl.svg)](https://github.com/nannou-org/hotglsl/blob/master/LICENSE-MIT) [![docs.rs](https://docs.rs/hotglsl/badge.svg)](https://docs.rs/hotglsl/)

A simple crate for hotloading GLSL shaders as SPIR-V.

```rust
fn main() {
    let shader_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("shaders");
    let watch = hotglsl::watch(&shader_dir).unwrap();
    println!("Edit the shaders in `examples/shaders/`!");
    loop {
        watch.await_event().unwrap();

        // On some OSes, a whole bunch of events will occur at once. Wait for
        // this to happen to avoid compiling our shader(s) twice unnecessarily.
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Compile each shader that has been touched and produce the result.
        for (path, result) in watch.compile_touched().unwrap() {
            println!("Tried compiling {:?}:", path);
            match result {
                Ok(_spirv_bytes) => println!("  Success!"),
                Err(e) => println!("  Woopsie!\n{}", e),
            }
        }
    }
}
```

Allows for watching one or more file and/or directory paths for GLSL shader file
changes.

See the `GLSL_EXTENSIONS` const for supported GLSL extensions that will be
watched. A `hotglsl::Watch` will ignore all events that don't involve a file
with one of these extensions.

Uses the `notify` crate for file system events and the `glsl-to-spirv` crate for
GLSL->SPIR-V compilation.
