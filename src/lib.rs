// extern crate proc_macro;
extern crate qstring;
extern crate ruisutil;

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

pub use qstring::QString;
pub use req::Request;
pub use req::Response;
pub use res::Context;

mod req;
mod res;

#[cfg(test)]
mod tests {
    use std::{mem, thread};

    use qstring::QString;

    use crate::{Engine, Request, Response};

    /* #[test]
    fn it_works() {
        println!("hello test");
        let ctx1 = ruisutil::Context::background(None);
        let ctx2 = ruisutil::Context::background(Some(ctx1.clone()));
        println!("start:ctx1:{},ctx2:{}", ctx1.done(), ctx2.done());
        ctx2.stop();
        println!("end:ctx1:{},ctx2:{}", ctx1.done(), ctx2.done());

        let wg = ruisutil::WaitGroup::new();
        let wg1 = wg.clone();
        thread::spawn(move || {
            let mut info = MsgInfo::new();
            info.version = 1;
            info.control = 2;
            info.lenCmd = 1000;
            let bts = ruisutil::struct2byte(&info);
            let ln = std::mem::size_of::<MsgInfo>();
            println!(
                "MsgInfo info.v:{},bts({}/{}):{:?}",
                info.version,
                bts.len(),
                ln,
                bts
            );
            let mut infos = MsgInfo::new();
            if let Ok(()) = ruisutil::byte2struct(&mut infos, bts) {
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
            Ok(mut res) => {
                println!("res code:{}", res.get_code());
                if let Some(bs) = res.get_bodys(None) {
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

// #[macro_export]
/* #[proc_macro_attribute]
pub fn controller(args: TokenStream, input: TokenStream) -> TokenStream {
    // parse the input
    let input = parse_macro_input!(input as ItemFn);
    // parse the arguments
    let mut args = parse_macro_input!(args as Args);
} */

pub struct Engine {
    ctx: ruisutil::Context,
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
    pub fn new(ctx: Option<ruisutil::Context>, addr: &str) -> Self {
        Self {
            ctx: ruisutil::Context::background(ctx),
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
                        return Err(ruisutil::ioerr(e.to_string(), None));
                    }
                }
            }
        }
        Ok(())
    }
    fn run_cli(&self, mut conn: TcpStream) {
        match res::ParseContext(&self.ctx, conn) {
            Err(e) => println!("ParseContext err:{}", e),
            Ok(mut res) => {
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
