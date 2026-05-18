use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);

    let content_root = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("core_experiences/freven.vanilla/content"));

    let output_root = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| content_root.join("_compiled/vanilla_blocktypes_v1"));

    let compiled = freven_vanilla_authoring::compile_source_tree(&content_root)?;
    freven_vanilla_authoring::write_compiled_output(&output_root, &compiled)?;

    println!(
        "wrote {} generated files to {}",
        compiled.generated_files.len(),
        output_root.display()
    );

    for file in &compiled.generated_files {
        println!("- {}", file.relative_path);
    }

    Ok(())
}
