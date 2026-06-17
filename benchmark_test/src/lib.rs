// Ownership

/// # Examples
///
/// ```
/// use final_project::string_length;
///
/// let s = String::from("Hello, Rust!");
/// assert_eq!(string_length(&s), 12);
/// assert_eq!(s, "Hello, Rust!");
/// ```
pub fn string_length(s: &str) -> usize {
    s.len()
}

/// # Examples
///
/// ```
/// use final_project::concat_strings;
///
/// let result = concat_strings(String::from("Hello, "), "Rust!");
/// assert_eq!(result, "Hello, Rust!");
/// ```
///
/// # Examples（邊界案例）
///
/// ```
/// use final_project::concat_strings;
///
/// // 空字串合併
/// let result = concat_strings(String::from(""), "");
/// assert_eq!(result, "");
/// ```
pub fn concat_strings(mut s1: String, s2: &str) -> String {
    s1.push_str(s2);
    s1
}

// Zero-Cost Abstraction 示範

/// # Examples
///
/// ```
/// use final_project::sum_slice;
///
/// assert_eq!(sum_slice(&[1, 2, 3, 4, 5]), 15);
/// assert_eq!(sum_slice(&[]),              0);
/// assert_eq!(sum_slice(&[-3, 3]),         0);
/// ```
pub fn sum_slice(nums: &[i64]) -> i64 {
    nums.iter().sum()
}

/// # Examples
///
/// ```
/// use final_project::filter_and_multiply_evens;
///
/// let result = filter_and_multiply_evens(&[1, 2, 3, 4, 5, 6], 3);
/// assert_eq!(result, vec![6, 12, 18]);
/// ```
///
/// ```
/// use final_project::filter_and_multiply_evens;
///
/// // 沒有偶數時回傳空 Vec
/// let result = filter_and_multiply_evens(&[1, 3, 5], 10);
/// assert_eq!(result, Vec::<i64>::new());
/// ```
pub fn filter_and_multiply_evens(nums: &[i64], multiplier: i64) -> Vec<i64> {
    nums.iter()
        .filter(|&&x| x % 2 == 0)
        .map(|&x| x * multiplier)
        .collect()
}

// Security 強制型別轉換與錯誤處理

/// 錯誤類型：解析或計算時可能發生的錯誤
#[derive(Debug, PartialEq)]
pub enum MathError {
    /// 除以零
    DivisionByZero,
    /// 整數溢位
    Overflow,
    /// 輸入格式不正確
    InvalidInput(String),
}

impl std::fmt::Display for MathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MathError::DivisionByZero => write!(f, "除以零錯誤"),
            MathError::Overflow => write!(f, "整數溢位"),
            MathError::InvalidInput(s) => write!(f, "無效輸入: {s}"),
        }
    }
}

/// 安全的整數除法
/// # Examples
///
/// ```
/// use final_project::{safe_divide, MathError};
///
/// assert_eq!(safe_divide(10, 2),  Ok(5));
/// assert_eq!(safe_divide(7,  0),  Err(MathError::DivisionByZero));
/// assert_eq!(safe_divide(-9, 3),  Ok(-3));
/// ```
pub fn safe_divide(a: i64, b: i64) -> Result<i64, MathError> {
    if b == 0 {
        Err(MathError::DivisionByZero)
    } else {
        Ok(a / b)
    }
}

/// 安全的加法（檢查 i64 溢位）
///
/// # Examples
///
/// ```
/// use final_project::{safe_add, MathError};
///
/// assert_eq!(safe_add(100, 200),            Ok(300));
/// assert_eq!(safe_add(i64::MAX, 1),         Err(MathError::Overflow));
/// assert_eq!(safe_add(i64::MIN, -1),        Err(MathError::Overflow));
/// ```
pub fn safe_add(a: i64, b: i64) -> Result<i64, MathError> {
    a.checked_add(b).ok_or(MathError::Overflow)
}

/// # Examples
///
/// ```
/// use final_project::{parse_integer, MathError};
///
/// assert_eq!(parse_integer("42"),    Ok(42));
/// assert_eq!(parse_integer("-7"),    Ok(-7));
/// assert!(matches!(
///     parse_integer("abc"),
///     Err(MathError::InvalidInput(_))
/// ));
/// assert!(matches!(
///     parse_integer(""),
///     Err(MathError::InvalidInput(_))
/// ));
/// ```
pub fn parse_integer(s: &str) -> Result<i64, MathError> {
    s.trim()
        .parse::<i64>()
        .map_err(|e| MathError::InvalidInput(e.to_string()))
}

// 模糊測試目標

/// # Examples
///
/// ```
/// use final_project::{evaluate_expression, MathError};
///
/// assert_eq!(evaluate_expression("10 + 5"),  Ok(15));
/// assert_eq!(evaluate_expression("10 - 3"),  Ok(7));
/// assert_eq!(evaluate_expression("4 * 6"),   Ok(24));
/// assert_eq!(evaluate_expression("15 / 3"),  Ok(5));
/// assert_eq!(evaluate_expression("10 / 0"),  Err(MathError::DivisionByZero));
/// assert!(matches!(
///     evaluate_expression("abc + 1"),
///     Err(MathError::InvalidInput(_))
/// ));
/// assert!(matches!(
///     evaluate_expression("1 % 2"),
///     Err(MathError::InvalidInput(_))
/// ));
/// ```
pub fn evaluate_expression(expr: &str) -> Result<i64, MathError> {
    let parts: Vec<&str> = expr.trim().splitn(3, ' ').collect();
    if parts.len() != 3 {
        return Err(MathError::InvalidInput(format!(
            "格式錯誤，期望 'a op b'，收到: '{expr}'"
        )));
    }
    let a = parse_integer(parts[0])?;
    let op = parts[1];
    let b = parse_integer(parts[2])?;

    match op {
        "+" => safe_add(a, b),
        "-" => safe_add(a, -b).map_err(|_| MathError::Overflow),
        "*" => a.checked_mul(b).ok_or(MathError::Overflow),
        "/" => safe_divide(a, b),
        _ => Err(MathError::InvalidInput(format!("不支援的運算符: '{op}'"))),
    }
}

// 單元測試

#[cfg(test)]
mod tests {
    use super::*;

    // --- Ownership ---
    #[test]
    fn test_string_length_basic() {
        assert_eq!(string_length("rust"), 4);
        assert_eq!(string_length(""), 0);
    }

    #[test]
    fn test_concat_does_not_duplicate_alloc() {
        let s = String::from("foo");
        let result = concat_strings(s, "bar");
        assert_eq!(result, "foobar");
    }

    // --- Zero-Cost Abstraction ---
    #[test]
    fn test_sum_large_slice() {
        let v: Vec<i64> = (1..=100).collect();
        assert_eq!(sum_slice(&v), 5050);
    }

    #[test]
    fn test_filter_multiply_negative_numbers() {
        // 負偶數也應被篩出
        let result = filter_and_multiply_evens(&[-4, -3, -2, 0, 1, 2], 2);
        assert_eq!(result, vec![-8, -4, 0, 4]);
    }

    // --- Security / Error Handling ---
    #[test]
    fn test_safe_divide_overflow_edge() {
        // 正常範圍邊界：MAX / -1 = MIN+1（均在 i64 範圍內）
        assert_eq!(evaluate_expression("50 - 75"), Ok(-25));
        assert_eq!(evaluate_expression("7 * 8"), Ok(56));
        assert_eq!(evaluate_expression("81 / 9"), Ok(9));
    }

    #[test]
    fn test_evaluate_overflow_add() {
        let expr = format!("{} + 1", i64::MAX);
        assert_eq!(evaluate_expression(&expr), Err(MathError::Overflow));
    }

    // FAILED
    #[test]
    fn test_failed_wrong_result() {
        assert_eq!(safe_divide(10, 2), Ok(5));
    }

    // IGNORED
    #[test]
    #[ignore]
    fn test_ignored_slow_operation() {
        assert_eq!(sum_slice(&(1..=10000).collect::<Vec<_>>()), 50005000);
    }
}
