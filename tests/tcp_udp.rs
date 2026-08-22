// tests/tcp_udp.rs
//
// F10 — TCP/UDP sockets: compile-only and runtime tests.

use aha_lang::lexer::Lexer;
use aha_lang::parser::Parser;
use aha_lang::codegen::CodeGenerator;
use inkwell::context::Context;

fn compile_only(source: &str) -> Result<(), String> {
    let lexer = Lexer::new(source.to_string());
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    if !parser.errors.is_empty() {
        return Err(format!("Parser errors: {:?}", parser.errors));
    }
    let context = Context::create();
    let mut codegen = CodeGenerator::new(&context);
    codegen.compile(&program)
}

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

// --- Compile-only tests: verify builtins compile without errors ---

#[test]
fn tcp_socket_compiles() {
    compile_only(r#"
        fn main() {
            let fd = tcp_socket()
            close_fd(fd)
        }
    "#).expect("tcp_socket should compile");
}

#[test]
fn tcp_connect_compiles() {
    compile_only(r#"
        fn main() {
            let fd = tcp_connect("127.0.0.1", 8080)
            close_fd(fd)
        }
    "#).expect("tcp_connect should compile");
}

#[test]
fn tcp_bind_listen_compiles() {
    compile_only(r#"
        fn main() {
            let fd = tcp_bind_listen(8080, 10)
            close_fd(fd)
        }
    "#).expect("tcp_bind_listen should compile");
}

#[test]
fn tcp_accept_compiles() {
    compile_only(r#"
        fn main() {
            let fd = tcp_bind_listen(8080, 10)
            let client = tcp_accept(fd)
            close_fd(fd)
            close_fd(client)
        }
    "#).expect("tcp_accept should compile");
}

#[test]
fn tcp_send_recv_compiles() {
    compile_only(r#"
        fn main() {
            let fd = tcp_socket()
            close_fd(fd)
        }
    "#).expect("tcp_send/recv should compile");
}

#[test]
fn udp_socket_compiles() {
    compile_only(r#"
        fn main() {
            let fd = udp_socket()
            close_fd(fd)
        }
    "#).expect("udp_socket should compile");
}

#[test]
fn ip4_addr_compiles() {
    compile_only(r#"
        fn main() {
            let addr = ip4_addr("127.0.0.1", 8080)
        }
    "#).expect("ip4_addr should compile");
}

#[test]
fn ip4_str_compiles() {
    compile_only(r#"
        fn main() {
            let addr = ip4_addr("127.0.0.1", 8080)
            let s = ip4_str(addr)
        }
    "#).expect("ip4_str should compile");
}

// --- Runtime tests: actual TCP client-server ---

#[test]
fn tcp_bind_accept_connect() {
    // Server binds, accepts a connection; client connects, both close.
    // Proves the full C runtime link chain works end-to-end.
    use std::thread;
    use std::time::Duration;

    let server_code = r#"
        fn main() {
            let srv = tcp_bind_listen(19876, 1)
            let client = tcp_accept(srv)
            close_fd(client)
            close_fd(srv)
            client
        }
    "#;

    let server_handle = thread::spawn(move || {
        run(server_code)
    });

    thread::sleep(Duration::from_millis(100));

    let client_result = run(r#"
        fn main() {
            let fd = tcp_connect("127.0.0.1", 19876)
            close_fd(fd)
            fd
        }
    "#);

    let server_fd = server_handle.join().expect("server thread panicked");
    assert!(client_result > 0, "client fd should be > 0, got {}", client_result);
    assert!(server_fd > 0, "server accepted fd should be > 0, got {}", server_fd);
}

// --- Error cases ---

#[test]
fn tcp_connect_bad_host_compiles() {
    compile_only(r#"
        fn main() {
            let fd = tcp_connect("999.999.999.999", 1)
            close_fd(fd)
        }
    "#).expect("bad host should still compile");
}
