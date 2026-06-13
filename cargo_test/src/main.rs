use final_project::{safe_add, safe_divide, string_length};

fn main() {
    let a = 10;
    let b = 0;
    let c = 60;
    println!("{}", string_length("Hello"));
    let div_result = safe_divide(a, b);
    println!("{}", div_result.unwrap_err());
    let add_result = safe_add(a, c);
    println!("{}", add_result.unwrap_err());
}
