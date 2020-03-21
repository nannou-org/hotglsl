fn main() {
    let shader_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("shaders");
    let watch = hotglsl::watch(&shader_dir).unwrap();
    println!("Edit the shaders in `examples/shaders/`!");
    loop {
        // Wait for some shader file event to occur.
        // Note: You only need to call this when you want to block, otherwise you can call
        // `compile_touched` and it will just yield nothing if nothing has changed.
        println!("Awaiting next event...");
        watch.await_event().unwrap();

        // On some OSes, a whole bunch of events will occur at once. Wait for this.
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Compile each touched shader and produce the result.
        for (path, result) in watch.compile_touched().unwrap() {
            println!("Tried compiling {:?}:", path);
            match result {
                Ok(_spirv_bytes) => println!("  Success!"),
                Err(e) => println!("  Woopsie!\n{}", e),
            }
        }
    }
}
