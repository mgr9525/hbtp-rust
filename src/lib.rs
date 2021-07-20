use std::{
    borrow::Cow,
    collections::HashMap,
    io, mem,
    net::{TcpListener, TcpStream},
    ptr, thread,
    time::Duration,
    usize,
};

pub mod bean;
pub mod util;

#[cfg(test)]
mod tests {
    use std::{mem, thread};

    use crate::{bean, util};

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
            let mut info = bean::MsgInfo::new();
            info.version = 1;
            info.control = 2;
            info.lenCmd = 1000;
            let bts = util::struct2byte(&info);
            println!("MsgInfo info.v:{},bts:{:?}", info.version, bts);
            let mut infos = bean::MsgInfo::new();
            if let Ok(()) = util::byte2struct(&mut infos, bts) {
                println!(
                    "MsgInfos infos.v:{},ctrl:{},cmdln:{}",
                    infos.version, infos.control, infos.lenCmd
                );
            }
            thread::sleep_ms(3000);
            println!("thread end!!!!!");
            drop(wg1);
        });
        println!("start wg.wait");
        wg.wait();
        println!("start wg.wait end!!!!!");
        thread::sleep_ms(500);
    }
}

pub struct Engine {
    ctx: util::Context,
    wg: util::WaitGroup,
    lsr: Option<TcpListener>,
    fns: HashMap<i32, fn(res: Context)>,
}
impl Drop for Engine {
    fn drop(&mut self) {
        self.lsr = None;
        self.ctx.stop();
        self.wg.wait();
        //self.lsr.
    }
}
impl Engine {
    pub fn start(ctx: Option<util::Context>, addr: &str) -> io::Result<Engine> {
        let lsr = TcpListener::bind(addr)?;
        let mut this = Engine {
            ctx: util::Context::background(ctx),
            wg: util::WaitGroup::new(),
            lsr: Some(lsr),
            fns: HashMap::new(),
        };
        let ptr = &this as *const Self as u64;
        thread::spawn(move || unsafe { &mut *(ptr as *mut Self) }.run());
        return Ok(this);
    }
    fn run(&self) {
        self.wg.clone();
        while !self.ctx.done() {
            if let Some(lsr) = &self.lsr {
                match lsr.accept() {
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
    }
    fn run_cli(&self, mut conn: TcpStream) {
        match self.ParseContext(&mut conn) {
            Err(e) => println!("ParseContext err:{}", e),
            Ok(res) => {
                if let Some(f) = self.fns.get(&res.control) {
                    f(res)
                }
            }
        }
    }

    fn ParseContext(&self, conn: &mut TcpStream) -> io::Result<Context> {
        let mut info = bean::MsgInfo::new();
        let infoln = mem::size_of::<bean::MsgInfo>();
        let ctx = util::Context::with_timeout(Some(self.ctx.clone()), Duration::from_secs(10));
        let bts = util::tcp_read(&ctx, conn, infoln)?;
        util::byte2struct(&mut info, &bts[..])?;
        if info.version != 1 {
            return Err(util::ioerrs("not found version!", None));
        }
        let mut rt = Context::new();
        let ctx = util::Context::with_timeout(Some(self.ctx.clone()), Duration::from_secs(30));
        if info.lenCmd > 0 {
            let bts = util::tcp_read(&ctx, conn, info.lenCmd as usize)?;
            rt.cmds = match String::from_utf8(bts.to_vec()) {
                Err(e) => return Err(util::ioerrs("cmd err", None)),
                Ok(v) => v,
            };
        }
        if info.lenArg > 0 {
            let bts = util::tcp_read(&ctx, conn, info.lenArg as usize)?;
            rt.args = match String::from_utf8(bts.to_vec()) {
                Err(e) => return Err(util::ioerrs("cmd err", None)),
                Ok(v) => v,
            };
        }
        if info.lenHead > 0 {
            let bts = util::tcp_read(&ctx, conn, info.lenHead as usize)?;
            rt.heads = Some(bts);
        }
        if info.lenBody > 0 {
            let bts = util::tcp_read(&ctx, conn, info.lenBody as usize)?;
            rt.bodys = Some(bts);
        }
        Ok(rt)
    }
}

pub struct Context {
    own: bool,
    conn: Option<TcpStream>,
    control: i32,
    cmds: String,
    args: String,
    heads: Option<Box<[u8]>>,
    bodys: Option<Box<[u8]>>,
}
impl Context {
    pub fn new() -> Self {
        Self {
            own: true,
            conn: None,
            control: 0,
            cmds: String::new(),
            args: String::new(),
            heads: None,
            bodys: None,
        }
    }
}
