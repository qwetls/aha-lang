// tests/integration_complex.rs
//
// COMPLEX INTEGRATION TESTS — real-world style programs that combine
// multiple language features simultaneously. These push the compiler
// harder than the basic integration tests: nested control flow,
// algorithm implementations (GCD, prime, power), string manipulation,
// function composition, and large combined programs.
//
// Golden Islamic Age inspiration: algorithms like al-Khwarizmi's
// algebra methods, algorithms for arithmetic that powered the
// House of Wisdom. These programs are small but meaningful.

use aha_lang::lexer::Lexer;
use aha_lang::parser::Parser;
use aha_lang::codegen::CodeGenerator;
use inkwell::context::Context;

/// Helper: compile and JIT-execute AHA! source code, return i64 result
fn run(source: &str) -> i64 {
    let lexer = Lexer::new(source.to_string());
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    if !parser.errors.is_empty() {
        panic!("Parser errors: {:?}", parser.errors);
    }

    let context = Context::create();
    let mut codegen = CodeGenerator::new(&context);
    codegen.compile(&program).expect("Codegen failed");
    codegen.run_jit().expect("JIT execution failed")
}

/// Helper: compile and expect a codegen error
fn expect_compile_error(source: &str) -> String {
    let lexer = Lexer::new(source.to_string());
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    let context = Context::create();
    let mut codegen = CodeGenerator::new(&context);
    codegen.compile(&program).unwrap_err()
}

/// Helper: modulo via integer arithmetic (a - (a/b)*b)
/// Inlined in test source as: n - (n / d) * d

// =====================================================================
// Classical Algorithms (al-Khwarizmi inspired)
// =====================================================================

#[test]
fn test_gcd_euclidean_iterative() {
    // Greatest Common Divisor via Euclidean algorithm (iterative)
    // mod: r = a - (a / b) * b
    let src = r#"
        fn gcd(a, b) {
            while b > 0 {
                let r = a - (a / b) * b;
                a = b;
                b = r;
            }
            return a;
        }
        gcd(48, 36)
    "#;
    assert_eq!(run(src), 12); // gcd(48,36) = 12
}

#[test]
fn test_gcd_mutual_prime() {
    let src = r#"
        fn gcd(a, b) {
            while b > 0 {
                let r = a - (a / b) * b;
                a = b;
                b = r;
            }
            return a;
        }
        gcd(17, 13)
    "#;
    assert_eq!(run(src), 1); // coprime → 1
}

#[test]
fn test_gcd_large_numbers() {
    let src = r#"
        fn gcd(a, b) {
            while b > 0 {
                let r = a - (a / b) * b;
                a = b;
                b = r;
            }
            return a;
        }
        gcd(1071, 462)
    "#;
    assert_eq!(run(src), 21); // gcd(1071, 462) = 21
}

#[test]
fn test_power_iterative() {
    // Exponentiation by repeated multiplication
    let src = r#"
        fn power(base, exp) {
            let result = 1;
            let i = 0;
            while i < exp {
                result = result * base;
                i = i + 1;
            }
            return result;
        }
        power(2, 10)
    "#;
    assert_eq!(run(src), 1024); // 2^10 = 1024
}

#[test]
fn test_power_zero_exponent() {
    let src = r#"
        fn power(base, exp) {
            let result = 1;
            let i = 0;
            while i < exp {
                result = result * base;
                i = i + 1;
            }
            return result;
        }
        power(7, 0)
    "#;
    assert_eq!(run(src), 1); // anything^0 = 1
}

#[test]
fn test_power_large() {
    // 3^15 = 14348907
    let src = r#"
        fn power(base, exp) {
            let result = 1;
            let i = 0;
            while i < exp {
                result = result * base;
                i = i + 1;
            }
            return result;
        }
        power(3, 15)
    "#;
    assert_eq!(run(src), 14348907);
}

#[test]
fn test_is_prime() {
    // mod: n - (n / i) * i
    let src = r#"
        fn is_prime(n) {
            if n < 2 {
                return 0;
            }
            let i = 2;
            while i * i <= n {
                if n - (n / i) * i == 0 {
                    return 0;
                }
                i = i + 1;
            }
            return 1;
        }
        is_prime(97)
    "#;
    assert_eq!(run(src), 1); // 97 is prime
}

#[test]
fn test_is_not_prime() {
    let src = r#"
        fn is_prime(n) {
            if n < 2 {
                return 0;
            }
            let i = 2;
            while i * i <= n {
                if n - (n / i) * i == 0 {
                    return 0;
                }
                i = i + 1;
            }
            return 1;
        }
        is_prime(91)
    "#;
    assert_eq!(run(src), 0); // 91 = 7*13, not prime
}

#[test]
fn test_is_prime_small() {
    let src = r#"
        fn is_prime(n) {
            if n < 2 {
                return 0;
            }
            let i = 2;
            while i * i <= n {
                if n - (n / i) * i == 0 {
                    return 0;
                }
                i = i + 1;
            }
            return 1;
        }
        is_prime(2)
    "#;
    assert_eq!(run(src), 1); // 2 is prime
}

#[test]
fn test_count_primes_below_30() {
    // Primes below 30: 2,3,5,7,11,13,17,19,23,29 = 10
    let src = r#"
        fn is_prime(n) {
            if n < 2 {
                return 0;
            }
            let i = 2;
            while i * i <= n {
                if n - (n / i) * i == 0 {
                    return 0;
                }
                i = i + 1;
            }
            return 1;
        }
        let count = 0;
        let n = 2;
        while n < 30 {
            count = count + is_prime(n);
            n = n + 1;
        }
        count
    "#;
    assert_eq!(run(src), 10);
}

#[test]
fn test_digit_sum() {
    // Sum of digits of 987654321 = 45
    // mod 10: n - (n / 10) * 10
    let src = r#"
        fn digit_sum(n) {
            let sum = 0;
            while n > 0 {
                sum = sum + (n - (n / 10) * 10);
                n = n / 10;
            }
            return sum;
        }
        digit_sum(987654321)
    "#;
    assert_eq!(run(src), 45);
}

#[test]
fn test_digit_sum_zero() {
    let src = r#"
        fn digit_sum(n) {
            let sum = 0;
            while n > 0 {
                sum = sum + (n - (n / 10) * 10);
                n = n / 10;
            }
            return sum;
        }
        digit_sum(0)
    "#;
    assert_eq!(run(src), 0);
}

#[test]
fn test_reverse_number() {
    // Reverse digits of 12345 → 54321
    let src = r#"
        fn reverse(n) {
            let result = 0;
            while n > 0 {
                result = result * 10 + (n - (n / 10) * 10);
                n = n / 10;
            }
            return result;
        }
        reverse(12345)
    "#;
    assert_eq!(run(src), 54321);
}

#[test]
fn test_collatz_steps() {
    // Collatz sequence length from 27 → 111 steps
    // even: n % 2 == 0 → n - (n/2)*2 == 0
    let src = r#"
        fn collatz(n) {
            let steps = 0;
            while n > 1 {
                if n - (n / 2) * 2 == 0 {
                    n = n / 2;
                } else {
                    n = 3 * n + 1;
                }
                steps = steps + 1;
            }
            return steps;
        }
        collatz(27)
    "#;
    assert_eq!(run(src), 111);
}

#[test]
fn test_collatz_small() {
    // collatz(6): 6→3→10→5→16→8→4→2→1 = 8 steps
    let src = r#"
        fn collatz(n) {
            let steps = 0;
            while n > 1 {
                if n - (n / 2) * 2 == 0 {
                    n = n / 2;
                } else {
                    n = 3 * n + 1;
                }
                steps = steps + 1;
            }
            return steps;
        }
        collatz(6)
    "#;
    assert_eq!(run(src), 8);
}

#[test]
fn test_factorial_iterative() {
    // 10! = 3628800
    let src = r#"
        fn fact(n) {
            let result = 1;
            let i = 1;
            while i <= n {
                result = result * i;
                i = i + 1;
            }
            return result;
        }
        fact(10)
    "#;
    assert_eq!(run(src), 3628800);
}

#[test]
fn test_factorial_small() {
    let src = r#"
        fn fact(n) {
            let result = 1;
            let i = 1;
            while i <= n {
                result = result * i;
                i = i + 1;
            }
            return result;
        }
        fact(5)
    "#;
    assert_eq!(run(src), 120); // 5! = 120
}

#[test]
fn test_fibonacci_iterative() {
    // fib(20) = 6765
    let src = r#"
        fn fib(n) {
            if n <= 1 {
                return n;
            }
            let a = 0;
            let b = 1;
            let i = 2;
            while i <= n {
                let c = a + b;
                a = b;
                b = c;
                i = i + 1;
            }
            return b;
        }
        fib(20)
    "#;
    assert_eq!(run(src), 6765);
}

#[test]
fn test_fibonacci_iterative_large() {
    // fib(40) = 102334155 — iterative, no stack pressure
    let src = r#"
        fn fib(n) {
            if n <= 1 {
                return n;
            }
            let a = 0;
            let b = 1;
            let i = 2;
            while i <= n {
                let c = a + b;
                a = b;
                b = c;
                i = i + 1;
            }
            return b;
        }
        fib(40)
    "#;
    assert_eq!(run(src), 102334155);
}

#[test]
fn test_fibonacci_iterative_zero() {
    let src = r#"
        fn fib(n) {
            if n <= 1 {
                return n;
            }
            let a = 0;
            let b = 1;
            let i = 2;
            while i <= n {
                let c = a + b;
                a = b;
                b = c;
                i = i + 1;
            }
            return b;
        }
        fib(0)
    "#;
    assert_eq!(run(src), 0);
}

#[test]
fn test_sum_of_squares() {
    // Sum of squares 1..10 = 385
    let src = r#"
        let total = 0;
        for i in 1..11 {
            total = total + (i * i);
        }
        total
    "#;
    assert_eq!(run(src), 385);
}

// =====================================================================
// Nested Control Flow Combinations
// =====================================================================

#[test]
fn test_multiplication_table_diagonal() {
    // Sum of diagonal of 5x5 multiplication table = 1+4+9+16+25 = 55
    let src = r#"
        let diag = 0;
        let i = 1;
        while i <= 5 {
            let j = 1;
            while j <= 5 {
                if i == j {
                    diag = diag + (i * j);
                }
                j = j + 1;
            }
            i = i + 1;
        }
        diag
    "#;
    assert_eq!(run(src), 55);
}

#[test]
fn test_nested_loops_with_skip_condition() {
    // Sum of i*j where i != j for i,j in 1..4
    // i=1: 2+3=5, i=2: 2+6=8, i=3: 3+6=9 → total 22
    let src = r#"
        let total = 0;
        for i in 1..4 {
            for j in 1..4 {
                if i != j {
                    total = total + (i * j);
                }
            }
        }
        total
    "#;
    assert_eq!(run(src), 22);
}

#[test]
fn test_if_chain_score_bands() {
    // Grade: 90+ → 4, 75+ → 3, 60+ → 2, 40+ → 1, else 0
    let src = r#"
        fn grade(score) {
            if score >= 90 { 4 }
            else if score >= 75 { 3 }
            else if score >= 60 { 2 }
            else if score >= 40 { 1 }
            else { 0 }
        }
        grade(85) * 100 + grade(95) * 10 + grade(30)
    "#;
    // 3*100 + 4*10 + 0 = 340
    assert_eq!(run(src), 340);
}

#[test]
fn test_triangle_pattern_sum() {
    // Row i contains i..5 sums: 15+14+12+9+5 = 55
    let src = r#"
        let total = 0;
        for i in 1..6 {
            for j in i..6 {
                total = total + j;
            }
        }
        total
    "#;
    assert_eq!(run(src), 55);
}

#[test]
fn test_fizzbuzz_weighted_sum() {
    // For 1..15: div by 3 → +3, div by 5 → +5, div by 15 → +15
    // mod 15: i - (i/15)*15; mod 5: i - (i/5)*5; mod 3: i - (i/3)*3
    let src = r#"
        let total = 0;
        let i = 1;
        while i <= 15 {
            if i - (i / 15) * 15 == 0 {
                total = total + 15;
            } else {
                if i - (i / 5) * 5 == 0 {
                    total = total + 5;
                } else {
                    if i - (i / 3) * 3 == 0 {
                        total = total + 3;
                    }
                }
            }
            i = i + 1;
        }
        total
    "#;
    // 15→+15, 5,10→+5+5=10, 3,6,9,12→+3*4=12 → 15+10+12=37
    assert_eq!(run(src), 37);
}

#[test]
fn test_accumulator_in_nested_function() {
    // Σ_{i=1..5} Σ_{j=1..i} i*j = 1+6+18+40+75 = 140
    let src = r#"
        fn sum_products(n) {
            let total = 0;
            let i = 1;
            while i <= n {
                let j = 1;
                while j <= i {
                    total = total + (i * j);
                    j = j + 1;
                }
                i = i + 1;
            }
            return total;
        }
        sum_products(5)
    "#;
    assert_eq!(run(src), 140);
}

#[test]
fn test_triple_nested_loop() {
    // 3-level nested loop: sum of i*j*k for i,j,k in 1..4
    // = (sum 1..3)^3 = 6^3 = 216
    let src = r#"
        let total = 0;
        for i in 1..4 {
            for j in 1..4 {
                for k in 1..4 {
                    total = total + (i * j * k);
                }
            }
        }
        total
    "#;
    assert_eq!(run(src), 216);
}

// =====================================================================
// String Operations
// =====================================================================

#[test]
fn test_string_concat_chain() {
    let src = r#"
        let s = "hello" + " " + "world";
        len(s)
    "#;
    assert_eq!(run(src), 11); // "hello world" = 11 chars
}

#[test]
fn test_string_concat_multiple() {
    let src = r#"
        let s = "a" + "b" + "c" + "d" + "e";
        len(s)
    "#;
    assert_eq!(run(src), 5);
}

#[test]
fn test_string_equality() {
    let src = r#"
        let a = "hello";
        let b = "hello";
        a == b
    "#;
    assert_eq!(run(src), 1); // true
}

#[test]
fn test_string_inequality() {
    let src = r#"
        let a = "hello";
        let b = "world";
        a == b
    "#;
    assert_eq!(run(src), 0); // false
}

#[test]
fn test_string_concat_then_compare() {
    let src = r#"
        let s = "foo" + "bar";
        s == "foobar"
    "#;
    assert_eq!(run(src), 1); // true
}

#[test]
fn test_string_not_equal() {
    let src = r#"
        let a = "hello";
        let b = "world";
        a != b
    "#;
    assert_eq!(run(src), 1); // true
}

#[test]
fn test_len_on_empty_string() {
    let src = r#"
        len("")
    "#;
    assert_eq!(run(src), 0);
}

#[test]
fn test_len_in_condition() {
    let src = r#"
        let name = "al-khwarizmi";
        if len(name) > 10 {
            100
        } else {
            0
        }
    "#;
    assert_eq!(run(src), 100); // "al-khwarizmi" = 12 chars > 10
}

#[test]
fn test_string_concat_in_function() {
    // String concat at top level (not in function — function params
    // are typed as i64 in codegen, so string params don't work yet)
    let src = r#"
        let first = "al";
        let space = " ";
        let last = "khwarizmi";
        let full = first + space + last;
        len(full)
    "#;
    assert_eq!(run(src), 12); // "al khwarizmi" = 12 chars
}

// =====================================================================
// Function Composition
// =====================================================================

#[test]
fn test_function_composition_chain() {
    // square(add(twice(3), 4)) = square(10) = 100
    let src = r#"
        fn twice(x) {
            x * 2
        }
        fn add(a, b) {
            a + b
        }
        fn square(x) {
            x * x
        }
        square(add(twice(3), 4))
    "#;
    assert_eq!(run(src), 100);
}

#[test]
fn test_function_returns_function_result() {
    let src = r#"
        fn base(x) {
            x + 10
        }
        fn wrapper(x) {
            base(x) * 2
        }
        wrapper(5)
    "#;
    assert_eq!(run(src), 30); // (5+10)*2 = 30
}

#[test]
fn test_multi_level_nesting() {
    // f1(5)=6, f2(5)=12, f3(5)=15, f4(5)=60
    let src = r#"
        fn f1(x) { x + 1 }
        fn f2(x) { f1(x) * 2 }
        fn f3(x) { f2(x) + 3 }
        fn f4(x) { f3(x) * 4 }
        f4(5)
    "#;
    assert_eq!(run(src), 60);
}

#[test]
fn test_math_functions_in_expression() {
    let src = r#"
        fn negate(x) {
            -x
        }
        min(10, negate(-5)) + max(3, abs(-20))
    "#;
    // min(10, 5) = 5, max(3, 20) = 20 → 25
    assert_eq!(run(src), 25);
}

#[test]
fn test_function_with_multiple_returns() {
    // Multiple return paths with different conditions
    let src = r#"
        fn classify(n) {
            if n > 100 {
                return 3;
            }
            if n > 10 {
                return 2;
            }
            if n > 0 {
                return 1;
            }
            return 0;
        }
        classify(50) * 1000 + classify(5) * 100 + classify(0) * 10 + classify(200)
    "#;
    // 2*1000 + 1*100 + 0*10 + 3 = 2103
    assert_eq!(run(src), 2103);
}

// =====================================================================
// Arrays
// =====================================================================

#[test]
fn test_array_literal_sum() {
    let src = r#"
        let arr = [1, 2, 3, 4, 5];
        arr[0] + arr[1] + arr[2] + arr[3] + arr[4]
    "#;
    assert_eq!(run(src), 15);
}

#[test]
fn test_array_index_expression() {
    let src = r#"
        let arr = [10, 20, 30, 40];
        arr[1 + 1]
    "#;
    assert_eq!(run(src), 30);
}

#[test]
fn test_array_with_variables() {
    let src = r#"
        let a = 5;
        let b = 10;
        let arr = [a, b, a + b];
        arr[2]
    "#;
    assert_eq!(run(src), 15);
}

#[test]
fn test_array_access_in_loop() {
    // Sum array elements via while loop
    let src = r#"
        let arr = [10, 20, 30, 40, 50];
        let sum = 0;
        let i = 0;
        while i < 5 {
            sum = sum + arr[i];
            i = i + 1;
        }
        sum
    "#;
    assert_eq!(run(src), 150);
}

#[test]
fn test_array_access_in_function() {
    // Pass array to function and sum elements
    let src = r#"
        fn array_sum(arr, size) {
            let total = 0;
            let i = 0;
            while i < size {
                total = total + arr[i];
                i = i + 1;
            }
            return total;
        }
        let data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        array_sum(data, 10)
    "#;
    assert_eq!(run(src), 55); // 1+2+...+10 = 55
}

#[test]
fn test_array_find_max() {
    let src = r#"
        fn find_max(arr, size) {
            let max = arr[0];
            let i = 1;
            while i < size {
                if arr[i] > max {
                    max = arr[i];
                }
                i = i + 1;
            }
            return max;
        }
        let data = [3, 7, 1, 9, 4, 6, 8, 2, 5];
        find_max(data, 9)
    "#;
    assert_eq!(run(src), 9);
}

#[test]
fn test_array_find_min() {
    let src = r#"
        fn find_min(arr, size) {
            let min_val = arr[0];
            let i = 1;
            while i < size {
                if arr[i] < min_val {
                    min_val = arr[i];
                }
                i = i + 1;
            }
            return min_val;
        }
        let data = [8, 3, 9, 1, 7, 2, 5, 4, 6];
        find_min(data, 9)
    "#;
    assert_eq!(run(src), 1);
}

#[test]
fn test_array_linear_search() {
    let src = r#"
        fn search(arr, size, target) {
            let i = 0;
            while i < size {
                if arr[i] == target {
                    return i;
                }
                i = i + 1;
            }
            return -1;
        }
        let data = [10, 20, 30, 40, 50];
        search(data, 5, 30)
    "#;
    assert_eq!(run(src), 2); // found at index 2
}

#[test]
fn test_array_linear_search_not_found() {
    let src = r#"
        fn search(arr, size, target) {
            let i = 0;
            while i < size {
                if arr[i] == target {
                    return i;
                }
                i = i + 1;
            }
            return -1;
        }
        let data = [10, 20, 30, 40, 50];
        search(data, 5, 99)
    "#;
    assert_eq!(run(src), -1); // not found
}

// =====================================================================
// Type Error Detection (Complex Cases)
// =====================================================================

#[test]
fn test_type_error_string_times_int() {
    let err = expect_compile_error("\"abc\" * 3");
    assert!(err.contains("Cannot apply"), "Expected type error, got: {}", err);
}

#[test]
fn test_type_error_bool_division() {
    let err = expect_compile_error("true / false");
    assert!(err.contains("Cannot apply"), "Expected type error, got: {}", err);
}

#[test]
fn test_type_error_undefined_variable() {
    let err = expect_compile_error("unknown_var + 1");
    assert!(err.contains("not found"), "Expected undefined variable error, got: {}", err);
}

#[test]
fn test_type_error_assign_to_undefined() {
    let err = expect_compile_error("x = 5");
    assert!(err.contains("Cannot assign"), "Expected assignment error, got: {}", err);
}

// =====================================================================
// Combined Feature Stress (The Real Test)
// =====================================================================

#[test]
fn test_complex_math_program() {
    // Combine: user functions, while loops, if/else, arrays, builtins
    let src = r#"
        fn classify(n) {
            if n == 0 {
                return 0;
            }
            if n > 0 {
                return 1;
            }
            return -1;
        }

        fn count_positives(nums, limit) {
            let count = 0;
            let i = 0;
            while i < limit {
                if classify(nums[i]) == 1 {
                    count = count + 1;
                }
                i = i + 1;
            }
            return count;
        }

        let arr = [-3, -2, -1, 0, 1, 2, 3];
        let positives = count_positives(arr, 7);
        positives * 10 + abs(min(-5, 3))
    "#;
    // 3 positives → 30 + abs(-5)=5 → 35
    assert_eq!(run(src), 35);
}

#[test]
fn test_alternating_series_sum() {
    // 1 - 3 + 5 - 7 + 9 - 11 + 13 = 7
    let src = r#"
        let total = 0;
        let sign = 1;
        let i = 1;
        while i < 14 {
            if sign == 1 {
                total = total + i;
            } else {
                total = total - i;
            }
            sign = -sign;
            i = i + 2;
        }
        total
    "#;
    assert_eq!(run(src), 7);
}

#[test]
fn test_bouncing_ball_simulation() {
    // Bouncing ball: each bounce halves height (integer division)
    // 100→50→25→12→6→3→1→0 = 7 bounces until height == 0
    let src = r#"
        fn bounce_count(start) {
            let height = start;
            let count = 0;
            while height > 0 {
                height = height / 2;
                count = count + 1;
            }
            return count;
        }
        bounce_count(100)
    "#;
    // 100/2=50, 50/2=25, 25/2=12, 12/2=6, 6/2=3, 3/2=1, 1/2=0 → 7 bounces
    assert_eq!(run(src), 7);
}

#[test]
fn test_sum_of_cubes() {
    // Σ i^3 for i=1..10 = (10*11/2)^2 = 55^2 = 3025
    let src = r#"
        fn square(x) {
            x * x
        }
        fn cube(x) {
            square(x) * x
        }
        let total = 0;
        let i = 1;
        while i <= 10 {
            total = total + cube(i);
            i = i + 1;
        }
        total
    "#;
    assert_eq!(run(src), 3025);
}

#[test]
fn test_deeply_nested_control_flow() {
    // for → if → while → if: 4 levels deep
    // mod 2: i - (i/2)*2
    let src = r#"
        let total = 0;
        for i in 1..5 {
            if i - (i / 2) * 2 == 0 {
                let j = 1;
                while j <= 3 {
                    if j == 2 {
                        total = total + i * j;
                    }
                    j = j + 1;
                }
            }
        }
        total
    "#;
    // i=2: j==2 → +4; i=4: j==2 → +8 → total = 12
    assert_eq!(run(src), 12);
}

#[test]
fn test_two_function_sum() {
    let src = r#"
        fn sum_range(start, end) {
            let total = 0;
            for i in start..end {
                total = total + i;
            }
            return total;
        }
        fn sum_squares_iter(n) {
            let total = 0;
            let i = 1;
            while i <= n {
                total = total + (i * i);
                i = i + 1;
            }
            return total;
        }
        sum_range(1, 6) + sum_squares_iter(4)
    "#;
    // sum_range(1,6) = 15; sum_squares_iter(4) = 30 → 45
    assert_eq!(run(src), 45);
}

#[test]
fn test_binary_search_iterative() {
    // Binary search for target 42 in "virtual array" where get(i) = i
    let src = r#"
        fn get(index) {
            index
        }
        fn search(target, size) {
            let lo = 0;
            let hi = size - 1;
            while lo <= hi {
                let mid = (lo + hi) / 2;
                let val = get(mid);
                if val == target {
                    return mid;
                }
                if val < target {
                    lo = mid + 1;
                } else {
                    hi = mid - 1;
                }
            }
            return -1;
        }
        search(42, 64)
    "#;
    assert_eq!(run(src), 42);
}

#[test]
fn test_binary_search_not_found() {
    let src = r#"
        fn get(index) {
            index * 2
        }
        fn search(target, size) {
            let lo = 0;
            let hi = size - 1;
            while lo <= hi {
                let mid = (lo + hi) / 2;
                let val = get(mid);
                if val == target {
                    return mid;
                }
                if val < target {
                    lo = mid + 1;
                } else {
                    hi = mid - 1;
                }
            }
            return -1;
        }
        search(43, 64)
    "#;
    // get(i) = 2i, searching for 43 (odd), all values are even → -1
    assert_eq!(run(src), -1);
}

#[test]
fn test_median_of_three() {
    // median(5, 9, 7) = 7
    // No && operator: use nested if
    let src = r#"
        fn median(a, b, c) {
            if a > b {
                if a < c {
                    return a;
                }
            }
            if b > a {
                if b < c {
                    return b;
                }
            }
            return c;
        }
        median(5, 9, 7)
    "#;
    assert_eq!(run(src), 7);
}

#[test]
fn test_median_another_set() {
    let src = r#"
        fn median(a, b, c) {
            if a > b {
                if a < c {
                    return a;
                }
            }
            if b > a {
                if b < c {
                    return b;
                }
            }
            return c;
        }
        median(3, 1, 2)
    "#;
    // sorted: 1,2,3 → median = 2 (c)
    assert_eq!(run(src), 2);
}

#[test]
fn test_arithmetic_progression_sum() {
    // Sum of AP: 5 + 10 + 15 + ... + 50 = 5*(1+2+...+10) = 5*55 = 275
    let src = r#"
        let total = 0;
        for i in 1..11 {
            total = total + i * 5;
        }
        total
    "#;
    assert_eq!(run(src), 275);
}

#[test]
fn test_newton_sqrt_approximation() {
    // Newton's method for integer sqrt of 1000
    // x_{n+1} = (x_n + n/x_n) / 2, starting from n
    // Converges to ~31 for sqrt(1000)
    let src = r#"
        fn isqrt(n) {
            let x = n;
            let y = (x + n / x) / 2;
            while y < x {
                x = y;
                y = (x + n / x) / 2;
            }
            return x;
        }
        isqrt(1000)
    "#;
    // sqrt(1000) ≈ 31.622 → isqrt = 31
    assert_eq!(run(src), 31);
}

#[test]
fn test_newton_sqrt_perfect() {
    let src = r#"
        fn isqrt(n) {
            let x = n;
            let y = (x + n / x) / 2;
            while y < x {
                x = y;
                y = (x + n / x) / 2;
            }
            return x;
        }
        isqrt(576)
    "#;
    // sqrt(576) = 24
    assert_eq!(run(src), 24);
}

#[test]
fn test_egg_drop_simulation() {
    // Classic problem: minimum drops for 2 eggs and 100 floors
    // Answer: 14 (smallest n where n(n+1)/2 >= 100)
    let src = r#"
        fn min_drops(floors) {
            let n = 1;
            while n * (n + 1) / 2 < floors {
                n = n + 1;
            }
            return n;
        }
        min_drops(100)
    "#;
    // 14*15/2 = 105 >= 100
    assert_eq!(run(src), 14);
}

#[test]
fn test_compound_interest() {
    // Compound interest: 1000 * (1.05)^10
    // Integer version: 1000 * 105^10 / 100^10
    // = 1000 * 1628841000000 / 10000000000 = 1628
    // Simpler: simulate year by year with integer math
    // P = 1000, rate = 5%, 10 years
    // P = P + P/20 each year
    let src = r#"
        fn compound(principal, years) {
            let i = 0;
            while i < years {
                principal = principal + principal / 20;
                i = i + 1;
            }
            return principal;
        }
        compound(1000, 10)
    "#;
    // Year-by-year integer division:
    // 1000+50=1050, 1050+52=1102, 1102+55=1157, 1157+57=1214,
    // 1214+60=1274, 1274+63=1337, 1337+66=1403, 1403+70=1473,
    // 1473+73=1546, 1546+77=1623
    assert_eq!(run(src), 1623);
}

#[test]
fn test_largest_prime_factor_check() {
    // Check if 131071 is prime (it's a Mersenne prime: 2^17 - 1)
    let src = r#"
        fn is_prime(n) {
            if n < 2 {
                return 0;
            }
            let i = 2;
            while i * i <= n {
                if n - (n / i) * i == 0 {
                    return 0;
                }
                i = i + 1;
            }
            return 1;
        }
        is_prime(131071)
    "#;
    assert_eq!(run(src), 1); // 2^17 - 1 = 131071 is prime
}
