use std::{
    io, mem,
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};

use qstring::QString;

use crate::{res::*, util};

pub struct Request {
    ctx: Option<util::Context>,
    sended: bool,
    addr: String,
    conn: Option<TcpStream>,
    ctrl: i32,
    cmds: String,
    args: Option<QString>,

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
            cmds: String::new(),
            args: None,

            tmout: Duration::from_secs(30),
        }
    }
    pub fn newcmd(addr: &str, control: i32, s: &str) -> Self {
        let mut c = Self::new(addr, control);
        c.command(s);
        c
    }
    pub fn timeout(&mut self, ts: Duration) {
        self.tmout = ts;
    }
    pub fn command(&mut self, s: &str) {
        self.cmds = String::from(s);
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
    fn connect(&mut self) -> io::Result<TcpStream> {
        match self.addr.as_str().to_socket_addrs() {
            Err(e) => return Err(util::ioerrs(format!("parse:{}", e).as_str(), None)),
            Ok(mut v) => loop {
                if let Some(sa) = v.next() {
                    println!("getip:{}", sa);
                    if let Ok(conn) = TcpStream::connect_timeout(&sa, self.tmout.clone()) {
                        return Ok(conn);
                    }
                } else {
                    break;
                }
            },
        };
        Err(util::ioerrs("not found ip", None))
    }
    fn send(&mut self, hds: Option<&[u8]>, bds: Option<&[u8]>) -> io::Result<TcpStream> {
        let mut conn = self.connect()?; //TcpStream::connect_timeout(&addr, self.tmout.clone())?;
        if self.sended {
            return Err(util::ioerrs("already request!", None));
        }
        self.sended = true;
        let mut args = String::new();
        if let Some(v) = &self.args {
            args = v.to_string();
        }
        let mut reqs = MsgInfo::new();
        reqs.version = 1;
        reqs.control = self.ctrl;
        reqs.lenCmd = self.cmds.len() as u16;
        reqs.lenArg = args.len() as u16;
        if let Some(v) = hds {
            reqs.lenHead = v.len() as u32;
        }
        if let Some(v) = bds {
            reqs.lenBody = v.len() as u32;
        }
        let bts = util::struct2byte(&reqs);
        let ctx = util::Context::with_timeout(self.ctx.clone(), Duration::from_secs(10));
        util::tcp_write(&ctx, &mut conn, bts)?;
        if reqs.lenCmd > 0 {
            let bts = self.cmds.as_bytes();
            util::tcp_write(&ctx, &mut conn, bts)?;
        }
        if reqs.lenArg > 0 {
            let bts = args.as_bytes();
            util::tcp_write(&ctx, &mut conn, bts)?;
        }
        if let Some(v) = hds {
            let ctx = util::Context::with_timeout(self.ctx.clone(), Duration::from_secs(30));
            util::tcp_write(&ctx, &mut conn, v)?;
        }
        if let Some(v) = bds {
            let ctx = util::Context::with_timeout(self.ctx.clone(), Duration::from_secs(50));
            util::tcp_write(&ctx, &mut conn, v)?;
        }
        Ok(conn)
    }
    fn response(&self, mut conn: TcpStream) -> io::Result<Response> {
        let mut info = ResInfoV1::new();
        let infoln = mem::size_of::<ResInfoV1>();
        let ctx = util::Context::with_timeout(self.ctx.clone(), Duration::from_secs(10));
        let bts = util::tcp_read(&ctx, &mut conn, infoln)?;
        util::byte2struct(&mut info, &bts[..])?;
        if (info.lenHead) as u64 > MaxHeads {
            return Err(util::ioerrs("bytes2 out limit!!", None));
        }
        if (info.lenBody) as u64 > MaxBodys {
            return Err(util::ioerrs("bytes3 out limit!!", None));
        }
        let mut rt = Response::new();
        rt.code = info.code;
        let ctx = util::Context::with_timeout(self.ctx.clone(), Duration::from_secs(30));
        let lnsz = info.lenHead as usize;
        if lnsz > 0 {
            let bts = util::tcp_read(&ctx, &mut conn, lnsz as usize)?;
            rt.heads = Some(bts);
        }
        let ctx = util::Context::with_timeout(self.ctx.clone(), Duration::from_secs(50));
        let lnsz = info.lenBody as usize;
        if lnsz > 0 {
            let bts = util::tcp_read(&ctx, &mut conn, lnsz as usize)?;
            rt.bodys = Some(bts);
        }
        rt.conn = Some(conn);
        Ok(rt)
    }
    pub fn dors(&mut self, hds: Option<&[u8]>, bds: Option<&[u8]>) -> io::Result<Response> {
        let conn = self.send(hds, bds)?;
        self.response(conn)
    }
    pub fn donrs(&mut self, hds: Option<&[u8]>, bds: Option<&[u8]>) -> io::Result<()> {
        let conn = self.send(hds, bds)?;
        self.conn = Some(conn);
        Ok(())
    }
    pub fn res(&mut self) -> io::Result<Response> {
        if let Some(v) = std::mem::replace(&mut self.conn, None) {
            return self.response(v);
        }
        Err(util::ioerrs("send?", None))
    }
    pub fn do_bytes(&mut self, hds: Option<&[u8]>, bds: &[u8]) -> io::Result<Response> {
        self.dors(hds, Some(bds))
    }
    pub fn do_string(&mut self, hds: Option<&[u8]>, s: &str) -> io::Result<Response> {
        self.do_bytes(hds, s.as_bytes())
    }
}

pub struct Response {
    conn: Option<TcpStream>,

    code: i32,
    heads: Option<Box<[u8]>>,
    bodys: Option<Box<[u8]>>,
}
impl Response {
    fn new() -> Self {
        Self {
            conn: None,
            code: 0,
            heads: None,
            bodys: None,
        }
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
    pub fn get_code(&self) -> i32 {
        self.code
    }
    pub fn get_heads(&self) -> &Option<Box<[u8]>> {
        &self.heads
    }
    pub fn get_bodys(&self) -> &Option<Box<[u8]>> {
        &self.bodys
    }
}
