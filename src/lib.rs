// extern crate proc_macro;

use std::{
    borrow::Cow,
    collections::{HashMap, LinkedList},
    io, mem,
    net::{TcpListener, TcpStream},
    ptr, thread,
    time::{Duration, SystemTime},
    usize,
};

pub mod bean;
pub mod util;

#[cfg(test)]
mod tests {
    use std::{mem, thread};

    use crate::{bean, util, Engine};

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
            let ln = std::mem::size_of::<bean::MsgInfo>();
            println!(
                "MsgInfo info.v:{},bts({}/{}):{:?}",
                info.version,
                bts.len(),
                ln,
                bts
            );
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

    #[test]
    fn hbtp_server() {
        let mut serv = match Engine::start(None, "0.0.0.0:7030") {
            Err(e) => {
                println!("err:{}", e);
                return;
            }
            Ok(v) => v,
        };
        println!("hbtp serv start!!!");
        serv.reg_fun(1, testFun);
        serv.run();
    }
    fn testFun(c: &mut crate::Context) {
        println!("teestFun ctrl:{}", c.control())
    }
}
type ConnFun = fn(res: &mut Context);

pub const ResCodeOk: i32 = 1;
pub const ResCodeErr: i32 = 2;
pub const ResCodeAuth: i32 = 3;
pub const ResCodeNotFound: i32 = 4;

// #[macro_export]
/* #[proc_macro_attribute]
pub fn controller(args: TokenStream, input: TokenStream) -> TokenStream {
    // parse the input
    let input = parse_macro_input!(input as ItemFn);
    // parse the arguments
    let mut args = parse_macro_input!(args as Args);
} */

pub struct Engine {
    ctx: util::Context,
    lsr: Option<TcpListener>,
    fns: HashMap<i32, LinkedList<ConnFun>>,
}
impl Drop for Engine {
    fn drop(&mut self) {
        self.lsr = None;
        self.ctx.stop();
        //self.lsr.
    }
}
impl Engine {
    pub fn start(ctx: Option<util::Context>, addr: &str) -> io::Result<Engine> {
        let lsr = TcpListener::bind(addr)?;
        let mut this = Engine {
            ctx: util::Context::background(ctx),
            lsr: Some(lsr),
            fns: HashMap::new(),
        };
        // let ptr = &this as *const Self as u64;
        // thread::spawn(move || unsafe { &mut *(ptr as *mut Self) }.run());
        return Ok(this);
    }
    pub fn run(&self) -> io::Result<()> {
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
                        return Err(util::ioerrs(e.to_string().as_str(), None));
                    }
                }
            }
        }
        Ok(())
    }
    fn run_cli(&self, mut conn: TcpStream) {
        match self.ParseContext(&mut conn) {
            Err(e) => println!("ParseContext err:{}", e),
            Ok(mut res) => {
                res.conn = Some(conn);
                if let Some(ls) = self.fns.get(&res.control()) {
                    let mut itr = ls.iter();
                    loop {
                        if res.is_sended() {
                            break;
                        }
                        if let Some(f) = itr.next() {
                            f(&mut res);
                        } else {
                            break;
                        }
                    }

                    if !res.is_sended() {
                        res.res_string(ResCodeErr, "Unknown");
                    }
                } else {
                    println!("not found function:{}", res.control())
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
        let mut rt = Context::new(info.control);
        let ctx = util::Context::with_timeout(Some(self.ctx.clone()), Duration::from_secs(30));
        let lnsz = info.lenCmd as usize;
        if lnsz > 0 {
            let bts = util::tcp_read(&ctx, conn, lnsz)?;
            rt.cmds = match std::str::from_utf8(&bts[..]) {
                Err(e) => return Err(util::ioerrs("cmd err", None)),
                Ok(v) => String::from(v),
            };
        }
        let lnsz = info.lenArg as usize;
        if lnsz > 0 {
            let bts = util::tcp_read(&ctx, conn, lnsz as usize)?;
            rt.args = match std::str::from_utf8(&bts[..]) {
                Err(e) => return Err(util::ioerrs("args err", None)),
                Ok(v) => String::from(v),
            };
        }
        let ctx = util::Context::with_timeout(Some(self.ctx.clone()), Duration::from_secs(50));
        let lnsz = info.lenHead as usize;
        if lnsz > 0 {
            let bts = util::tcp_read(&ctx, conn, lnsz as usize)?;
            rt.heads = Some(bts);
        }
        let ctx = util::Context::with_timeout(Some(self.ctx.clone()), Duration::from_secs(100));
        let lnsz = info.lenBody as usize;
        if lnsz > 0 {
            let bts = util::tcp_read(&ctx, conn, lnsz as usize)?;
            rt.bodys = Some(bts);
        }
        Ok(rt)
    }

    pub fn reg_fun(&mut self, control: i32, f: ConnFun) {
        if let Some(v) = self.fns.get_mut(&control) {
            v.push_back(f);
        } else {
            let mut v = LinkedList::new();
            v.push_back(f);
            self.fns.insert(control, v);
        }
    }
}

pub struct Context {
    sended: bool,
    conn: Option<TcpStream>,
    ctrl: i32,
    cmds: String,
    args: String,
    heads: Option<Box<[u8]>>,
    bodys: Option<Box<[u8]>>,
}
impl Context {
    pub fn new(control: i32) -> Self {
        Self {
            sended: false,
            conn: None,
            ctrl: control,
            cmds: String::new(),
            args: String::new(),
            heads: None,
            bodys: None,
        }
    }
    pub fn get_conn(&self) -> &TcpStream {
        match &self.conn {
            Some(v) => v,
            None => panic!("not found conn!logic err"),
        }
    }
    pub fn control(&self) -> i32 {
        self.ctrl
    }
    pub fn command(&self) -> &str {
        self.cmds.as_str()
    }
    pub fn get_args(&self) -> String {
        self.args.clone()
    }
    pub fn get_heads(&self) -> &Option<Box<[u8]>> {
        &self.heads
    }
    pub fn get_bodys(&self) -> &Option<Box<[u8]>> {
        &self.bodys
    }
    pub fn is_sended(&self) -> bool {
        self.sended
    }

    pub fn response(
        &mut self,
        code: i32,
        hds: Option<&[u8]>,
        bds: Option<&[u8]>,
    ) -> io::Result<()> {
        let conn = match &mut self.conn {
            Some(v) => v,
            None => return Err(util::ioerrs("not found conn", None)),
        };
        if self.sended {
            return Err(util::ioerrs("already responsed!", None));
        }
        self.sended = true;
        let mut res = bean::ResInfoV1::new(code);
        if let Some(v) = hds {
            res.lenHead = v.len() as u32;
        }
        if let Some(v) = bds {
            res.lenBody = v.len() as u32;
        }
        let bts = util::struct2byte(&res);
        let ctx = util::Context::with_timeout(None, Duration::from_secs(10));
        util::tcp_write(&ctx, conn, bts)?;
        if let Some(v) = hds {
            let ctx = util::Context::with_timeout(None, Duration::from_secs(20));
            util::tcp_write(&ctx, conn, v)?;
        }
        if let Some(v) = bds {
            let ctx = util::Context::with_timeout(None, Duration::from_secs(30));
            util::tcp_write(&ctx, conn, v)?;
        }

        Ok(())
    }
    pub fn res_bytes(&mut self, code: i32, bds: &[u8]) -> io::Result<()> {
        self.response(code, None, Some(bds))
    }
    pub fn res_string(&mut self, code: i32, s: &str) -> io::Result<()> {
        self.res_bytes(code, s.as_bytes())
    }
}

pub struct Request {
    ctx: Option<util::Context>,
    sended: bool,
    addr: String,
    conn: Option<TcpStream>,
    ctrl: i32,
    cmds: Option<String>,
    args: Option<String>,

    code: i32,
    heads: Option<Box<[u8]>>,
    bodys: Option<Box<[u8]>>,

    started: SystemTime,
    tmout: Duration,
}
impl Request {
    pub fn new(addr: &str, control: i32) -> Self {
        Self {
            ctx: None,
            sended: false,
            addr: String::from(addr),
            conn: None,
            ctrl: control,
            cmds: None,
            args: None,

            code: 0,
            heads: None,
            bodys: None,

            started: SystemTime::UNIX_EPOCH,
            tmout: Duration::from_secs(120),
        }
    }
    pub fn timeout(mut self, ts: Duration) -> Self {
        self.tmout = ts;
        self
    }
}
