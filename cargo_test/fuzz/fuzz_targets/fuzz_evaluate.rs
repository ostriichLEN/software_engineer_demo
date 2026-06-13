#![no_main]

use libfuzzer_sys::fuzz_target;
use final_project::evaluate_expression;

fuzz_target!(|data: &[u8]| {
    // 嘗試將 fuzz 資料轉為 UTF-8 字串
    if let Ok(s) = std::str::from_utf8(data) {

        let _ = evaluate_expression(s);
    }
});
