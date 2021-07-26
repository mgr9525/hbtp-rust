// extern crate proc_macro;
extern crate qstring;

use std::{
    any,
    borrow::Cow,
    collections::{HashMap, LinkedList},
    io, mem,
    net::{TcpListener, TcpStream},
    ptr, thread,
    time::{Duration, SystemTime},
    usize,
};

use qstring::QString;
pub use req::Request;
pub use req::Response;

pub mod req;
pub mod util;

#[cfg(test)]
mod tests {
    use std::{mem, thread};

    use qstring::QString;

    use crate::{util, Engine, Request, Response};

    /* #[test]
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
            let mut info = MsgInfo::new();
            info.version = 1;
            info.control = 2;
            info.lenCmd = 1000;
            let bts = util::struct2byte(&info);
            let ln = std::mem::size_of::<MsgInfo>();
            println!(
                "MsgInfo info.v:{},bts({}/{}):{:?}",
                info.version,
                bts.len(),
                ln,
                bts
            );
            let mut infos = MsgInfo::new();
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
    } */

    #[test]
    fn hbtp_server() {
        let mut serv = Engine::new(None, "0.0.0.0:7030");
        println!("hbtp serv start!!!");
        serv.reg_fun(1, testFun);
        serv.run();
    }
    fn testFun(c: &mut crate::Context) {
        println!(
            "testFun ctrl:{},cmd:{},ishell:{},arg hello1:{}",
            c.control(),
            c.command(),
            c.command() == "hello",
            c.get_arg("hehe1").unwrap().as_str()
        );
        panic!("whats?");
        if let Err(e) = c.res_string(crate::ResCodeOk, "hello,there is rust!!") {
            println!("testFun res_string err:{}", e)
        };
    }
    #[test]
    fn hbtp_request() {
        let mut req = Request::new("localhost:7030", 1);
        req.command("hello");
        req.add_arg("hehe1", "123456789");
        match req.do_string(None, "dedededede") {
            Err(e) => println!("do err:{}", e),
            Ok(res) => {
                println!("res code:{}", res.get_code());
                if let Some(bs) = res.get_bodys() {
                    println!("res data:{}", std::str::from_utf8(&bs[..]).unwrap())
                }
            }
        };
    }
    #[test]
    fn qstring_test() {
        let mut qs = QString::from("foo=bar");
        qs.add_pair(("haha", "hehe"));
        let val = qs.get("foo").unwrap();
        println!("val:{},s:{}", val, qs.to_string());
    }
}
type ConnFun = fn(res: &mut Context);

pub const ResCodeOk: i32 = 1;
pub const ResCodeErr: i32 = 2;
pub const ResCodeAuth: i32 = 3;
pub const ResCodeNotFound: i32 = 4;

const MaxOther: u64 = 1024 * 1024 * 20; //20M
const MaxHeads: u64 = 1024 * 1024 * 100; //100M
const MaxBodys: u64 = 1024 * 1024 * 1024; //1G

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
    fns: HashMap<i32, LinkedList<ConnFun>>,
    addr: String,
    lsr: Option<TcpListener>,
}
impl Drop for Engine {
    fn drop(&mut self) {
        self.lsr = None;
        self.ctx.stop();
        //self.lsr.
    }
}
impl Engine {
    pub fn new(ctx: Option<util::Context>, addr: &str) -> Self {
        Self {
            ctx: util::Context::background(ctx),
            fns: HashMap::new(),
            addr: String::from(addr),
            lsr: None,
        }
    }
    pub fn stop(&mut self) {
        self.lsr = None;
        self.ctx.stop();
    }
    pub fn run(&mut self) -> io::Result<()> {
        let lsr = TcpListener::bind(self.addr.as_str())?;
        self.lsr = Some(lsr);
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
                    while let Some(f) = itr.next() {
                        if res.is_sended() {
                            break;
                        }
                        let ptr = &mut res as *mut Context;
                        let rst = std::panic::catch_unwind(move || {
                            let ts = unsafe { (&mut *ptr) as &mut Context };
                            f(ts);
                        });
                        if let Some(e) = rst.err() {
                            println!("ctrl command fun err:{:?}", e);
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
        let mut info = MsgInfo::new();
        let infoln = mem::size_of::<MsgInfo>();
        let ctx = util::Context::with_timeout(Some(self.ctx.clone()), Duration::from_secs(10));
        let bts = util::tcp_read(&ctx, conn, infoln)?;
        util::byte2struct(&mut info, &bts[..])?;
        if info.version != 1 {
            return Err(util::ioerrs("not found version!", None));
        }
        if (info.lenCmd + info.lenArg) as u64 > MaxOther {
            return Err(util::ioerrs("bytes1 out limit!!", None));
        }
        if (info.lenHead) as u64 > MaxHeads {
            return Err(util::ioerrs("bytes2 out limit!!", None));
        }
        if (info.lenBody) as u64 > MaxBodys {
            return Err(util::ioerrs("bytes3 out limit!!", None));
        }
        let mut rt = Context::new(info.control);
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
            let args = match std::str::from_utf8(&bts[..]) {
                Err(e) => return Err(util::ioerrs("args err", None)),
                Ok(v) => String::from(v),
            };
            rt.args = Some(QString::from(args.as_str()));
        }
        let ctx = util::Context::with_timeout(Some(self.ctx.clone()), Duration::from_secs(30));
        let lnsz = info.lenHead as usize;
        if lnsz > 0 {
            let bts = util::tcp_read(&ctx, conn, lnsz as usize)?;
            rt.heads = Some(bts);
        }
        let ctx = util::Context::with_timeout(Some(self.ctx.clone()), Duration::from_secs(50));
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
    args: Option<QString>,
    heads: Option<Box<[u8]>>,
    bodys: Option<Box<[u8]>>,

    data: HashMap<String, Vec<u8>>,
}
impl Context {
    pub fn new(control: i32) -> Self {
        Self {
            sended: false,
            conn: None,
            ctrl: control,
            cmds: String::new(),
            args: None,
            heads: None,
            bodys: None,
            data: HashMap::new(),
        }
    }
    pub fn get_data(&self, s: &str) -> Option<&Vec<u8>> {
        self.data.get(&String::from(s))
    }
    pub fn put_data(&mut self, s: &str, v: Vec<u8>) {
        self.data.insert(String::from(s), v);
    }
    pub fn get_conn(&self) -> &TcpStream {
        if let Some(v) = &self.conn {
            return v;
        }
        panic!("conn?");
    }
    pub fn own_conn(&mut self) -> TcpStream {
        if let Some(v) = std::mem::replace(&mut self.conn, None) {
            return v;
        }
        panic!("conn?");
    }
    pub fn control(&self) -> i32 {
        self.ctrl
    }
    pub fn command(&self) -> &str {
        self.cmds.as_str()
    }
    pub fn get_args<'a>(&'a self) -> Option<&'a QString> {
        if let Some(v) = &self.args {
            Some(v)
        } else {
            None
        }
    }
    pub fn get_arg(&self, name: &str) -> Option<String> {
        if let Some(v) = &self.args {
            if let Some(s) = v.get(name) {
                Some(String::from(s))
            } else {
                None
            }
        } else {
            None
        }
    }
    /* pub fn set_arg(&mut self, name: &str, value: &str) {
        if let None = &self.args {
            self.args = Some(QString::from(""));
        }
        self.args.unwrap().add_str(origin)
    } */
    pub fn add_arg(&mut self, name: &str, value: &str) {
        if let Some(v) = &mut self.args {
            v.add_pair((name, value));
        } else {
            self.args = Some(QString::new(vec![(name, value)]));
        }
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
        let mut res = ResInfoV1::new();
        res.code = code;
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

//----------------------------------bean
#[repr(C, packed)]
struct MsgInfo {
    pub version: u16,
    pub control: i32,
    pub lenCmd: u16,
    pub lenArg: u16,
    pub lenHead: u32,
    pub lenBody: u32,
}
impl MsgInfo {
    pub fn new() -> Self {
        Self {
            version: 0,
            control: 0,
            lenCmd: 0,
            lenArg: 0,
            lenHead: 0,
            lenBody: 0,
        }
    }
}
#[repr(C, packed)]
struct ResInfoV1 {
    pub code: i32,
    pub lenHead: u32,
    pub lenBody: u32,
}
impl ResInfoV1 {
    pub fn new() -> Self {
        Self {
            code: 0,
            lenHead: 0,
            lenBody: 0,
        }
    }
}
