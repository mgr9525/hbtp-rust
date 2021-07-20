use std::{
    io,
    net::{TcpListener, TcpStream},
    thread,
};

pub mod util;

#[cfg(test)]
mod tests {
    use std::thread;

    use crate::util;

    #[test]
    fn it_works() {
        println!("hello test");
        let ctx1 = util::Context::background(None);
        let ctx2 = util::Context::background(Some(ctx1.clone()));
        println!("start:ctx1:{},ctx2:{}", ctx1.done(), ctx2.done());
        ctx2.stop();
        println!("end:ctx1:{},ctx2:{}", ctx1.done(), ctx2.done());

        let wg = util::WaitGroup::new();
        let wg1 = wg.clone();
        thread::spawn(move || {
            thread::sleep_ms(5000);
            drop(wg1);
        });
        println!("start wg.wait");
        wg.wait();
    }
}

pub struct Engine {
    ctx: util::Context,
    lsr: TcpListener,
}
impl Engine {
    pub fn start(ctx: Option<util::Context>, addr: &str) -> io::Result<Engine> {
        let lsr = TcpListener::bind(addr)?;
        let mut this = Engine {
            ctx: util::Context::background(ctx),
            lsr: lsr,
        };
        let ptr = &this as *const Self as u64;
        thread::spawn(move || unsafe { &mut *(ptr as *mut Self) }.run());
        return Ok(this);
    }
    fn run(&self) {
        while !self.ctx.done() {
            match self.lsr.accept() {
                Ok((conn, addr)) => {
                    println!("accept conn addr:{}", &addr);
                    self.run_cli(conn);
                }
                Err(e) => {
                    println!("accept err:{}", e);
                    self.ctx.stop();
                }
            }
        }
    }
    fn run_cli(&self, conn: TcpStream) {}
}
