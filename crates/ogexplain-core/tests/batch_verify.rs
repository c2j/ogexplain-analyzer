//! Batch verification test - run with: cargo test -p ogexplain-core --test batch_verify -- --nocapture

use ogexplain_core::parse_multi;
use std::fs;

#[test]
fn batch_verify_examples() {
    let dir = "../../examples/gauss";
    if !std::path::Path::new(dir).exists() {
        eprintln!("Skipping: {} not found", dir);
        return;
    }

    let mut total = 0usize;
    let mut ok_files = 0usize;
    let mut ok_blocks = 0usize;
    let mut no_blocks = 0usize;
    let mut parse_err = 0usize;
    let mut first_errors: Vec<(String, String)> = Vec::new();

    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let p = e.path();
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            ext == "source" || ext == "out"
        })
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in &entries {
        let path = entry.path();
        total += 1;
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => {
                no_blocks += 1;
                continue;
            }
        };

        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        match parse_multi(&content) {
            Ok(plans) => {
                if plans.is_empty() {
                    no_blocks += 1;
                } else {
                    ok_files += 1;
                    ok_blocks += plans.len();
                }
            }
            Err(e) => {
                parse_err += 1;
                if first_errors.len() < 50 {
                    first_errors.push((name, format!("{}", e)));
                }
            }
        }
    }

    println!();
    println!("=== 批量解析验证结果 ===");
    println!("总文件数:       {}", total);
    println!(
        "成功解析:       {} 个文件 (共 {} 个 EXPLAIN 块)",
        ok_files, ok_blocks
    );
    println!("无 EXPLAIN 块:  {}", no_blocks);
    println!("解析失败:       {}", parse_err);

    if !first_errors.is_empty() {
        println!();
        println!("--- 失败详情 (前50个) ---");
        for (name, err) in &first_errors {
            println!("  {}: {}", name, err);
        }
    }

    // Don't fail the test - this is informational
    // But assert at least some files parse successfully
    assert!(
        ok_files > 0,
        "Expected at least some files to parse successfully"
    );
}
