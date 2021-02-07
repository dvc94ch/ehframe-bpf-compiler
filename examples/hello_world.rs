use backtrace::Backtrace;

fn main() {
    fill_my_stack1(10);
}

fn fill_my_stack1(depth: u8) {
    if depth == 0 {
        stack_filled();
    } else {
        fill_my_stack2(depth - 1);
    }
}

fn fill_my_stack2(depth: u8) {
    if depth == 0 {
        stack_filled();
    } else {
        fill_my_stack1(depth - 1);
    }
}

fn stack_filled() {
    println!("{:?}", Backtrace::new());

    let mut i = 0;
    stack_walker::walk_stack(|ctx| {
        let mut resolved = false;
        backtrace::resolve(ctx.ip() as *const std::ffi::c_void as *mut _, |symbol| {
            if !resolved {
                resolved = true;
                print!("{:4}: ", i);
                i += 1;
            } else {
                print!("      ");
            }
            println!("{:#}", symbol.name().unwrap());
            if let (Some(file_name), Some(line), Some(col)) =
                (symbol.filename(), symbol.lineno(), symbol.colno())
            {
                println!("             at {}:{}:{}", file_name.display(), line, col);
            }
        })
    })
    .unwrap();
}
