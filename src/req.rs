use std::{
    io, mem,
    net::{TcpStream, ToSocketAddrs},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use qstring::QString;

use crate::res::*;

pub struct Request {
    ctx: Option<ruisutil::Context>,
    sended: bool,
    addr: String,
    conn: Option<TcpStream>,
    ctrl: i32,
    cmds: String,
    args: Option<QString>,

    tmout: Duration,
    lmt_tm: LmtTmConfig,
    lmt_max: LmtMaxConfig,

    use_version: u16,
}
impl Request {
    const MINS: Duration = Duration::from_millis(100);
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
            lmt_tm: LmtTmConfig::default(),
            lmt_max: LmtMaxConfig::default(),

            use_version: 0,
        }
    }
    pub fn set_use_version(&mut self, v: u16) {
        self.use_version = v;
    }
    pub fn set_lmt_tm(&mut self, limit: LmtTmConfig) {
        self.lmt_tm = limit;
    }
    pub fn set_lmt_max(&mut self, limit: LmtMaxConfig) {
        self.lmt_max = limit;
    }
    pub fn newcmd(addr: &str, control: i32, s: &str) -> Self {
        let mut c = Self::new(addr, control);
        c.command(s);
        c
    }
    pub fn timeout(&mut self, ts: Duration) {
        if ts > Self::MINS {
            self.tmout = ts;
        }
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
            Err(e) => return Err(ruisutil::ioerr(format!("parse:{}", e), None)),
            Ok(mut v) => loop {
                if let Some(sa) = v.next() {
                    // println!("connect to ip:{}", sa);
                    if let Ok(conn) = TcpStream::connect_timeout(&sa, self.tmout.clone()) {
                        return Ok(conn);
                    }
                } else {
                    break;
                }
            },
        };
        Err(ruisutil::ioerr("not found ip", None))
    }
    fn send(&mut self, hds: Option<&[u8]>, bds: Option<&[u8]>) -> io::Result<TcpStream> {
        let mut conn = self.connect()?; //TcpStream::connect_timeout(&addr, self.tmout.clone())?;
        if self.sended {
            return Err(ruisutil::ioerr("already request!", None));
        }
        self.sended = true;
        let mut args = String::new();
        if let Some(v) = &self.args {
            args = v.to_string();
        }
        let mut reqs = MsgInfo::new();
        reqs.version = 2;
        reqs.control = self.ctrl;
        reqs.len_cmd = self.cmds.len() as u16;
        reqs.len_arg = args.len() as u16;
        if let Some(v) = hds {
            reqs.len_head = v.len() as u32;
        }
        if let Some(v) = bds {
            reqs.len_body = v.len() as u32;
        }
        if self.use_version > 0 {
            reqs.version = self.use_version;
        }
        let bts = ruisutil::struct2byte(&reqs);
        let ctx = ruisutil::Context::with_timeout(self.ctx.clone(), self.lmt_tm.tm_ohther);
        ruisutil::tcp_write(&ctx, &mut conn, bts)?;
        if reqs.version >= 2 {
            ruisutil::tcp_write(&ctx, &mut conn, &[0x48, 0x42, 0x54, 0x50])?;
        }
        if reqs.len_cmd > 0 {
            let bts = self.cmds.as_bytes();
            ruisutil::tcp_write(&ctx, &mut conn, bts)?;
        }
        if reqs.len_arg > 0 {
            let bts = args.as_bytes();
            ruisutil::tcp_write(&ctx, &mut conn, bts)?;
        }
        if let Some(v) = hds {
            let ctx = ruisutil::Context::with_timeout(self.ctx.clone(), Duration::from_secs(30));
            ruisutil::tcp_write(&ctx, &mut conn, v)?;
        }
        if let Some(v) = bds {
            let ctx = ruisutil::Context::with_timeout(self.ctx.clone(), Duration::from_secs(50));
            ruisutil::tcp_write(&ctx, &mut conn, v)?;
        }
        Ok(conn)
    }
    fn response(&self, mut conn: TcpStream) -> io::Result<Response> {
        let mut info = ResInfoV1::new();
        let infoln = mem::size_of::<ResInfoV1>();
        let ctx = ruisutil::Context::with_timeout(self.ctx.clone(), self.lmt_tm.tm_ohther);
        let bts = ruisutil::tcp_read(&ctx, &mut conn, infoln)?;
        ruisutil::byte2struct(&mut info, &bts[..])?;
        if (info.len_head) as u64 > MaxHeads {
            return Err(ruisutil::ioerr("bytes2 out limit!!", None));
        }
        let heads;
        let ctx = ruisutil::Context::with_timeout(self.ctx.clone(), self.lmt_tm.tm_heads);
        let lnsz = info.len_head as usize;
        if lnsz > 0 {
            let bts = ruisutil::tcp_read(&ctx, &mut conn, lnsz as usize)?;
            heads = Some(ruisutil::bytes::ByteBox::from(bts));
        } else {
            heads = None;
        }
        /* let ctx = ruisutil::Context::with_timeout(self.ctx.clone(), Duration::from_secs(50));
        let lnsz = info.len_body as usize;
        if lnsz > 0 {
            let bts = ruisutil::tcp_read(&ctx, &mut conn, lnsz as usize)?;
            rt.bodys = Some(bts);
        } */
        Ok(Response::new(
            conn,
            info.code,
            heads,
            info.len_body as usize,
        ))
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
        Err(ruisutil::ioerr("send?", None))
    }
    pub fn do_bytes(&mut self, hds: Option<&[u8]>, bds: &[u8]) -> io::Result<Response> {
        self.dors(hds, Some(bds))
    }
    pub fn do_string(&mut self, hds: Option<&[u8]>, s: &str) -> io::Result<Response> {
        self.do_bytes(hds, s.as_bytes())
    }
}

#[derive(Clone)]
pub struct Response {
    inner: ruisutil::ArcMut<Inner>,
}
struct Inner {
    conn: Option<TcpStream>,

    code: i32,
    heads: Option<ruisutil::bytes::ByteBox>,
    bodys: Option<ruisutil::bytes::ByteBox>,
    bodyok: AtomicBool,
    bodylen: usize,
}
impl Response {
    fn new(
        conn: TcpStream,
        code: i32,
        heads: Option<ruisutil::bytes::ByteBox>,
        byln: usize,
    ) -> Self {
        Self {
            inner: ruisutil::ArcMut::new(Inner {
                conn: Some(conn),
                code: code,
                heads: heads,
                bodys: None,
                bodyok: AtomicBool::new(false),
                bodylen: byln,
            }),
        }
    }
    pub fn get_conn(&self) -> &TcpStream {
        if let Some(v) = &self.inner.conn {
            return v;
        }
        panic!("conn?");
    }
    pub fn own_conn(&self) -> TcpStream {
        self.get_bodys(None);
        let ins = unsafe { self.inner.muts() };
        if let Some(v) = std::mem::replace(&mut ins.conn, None) {
            return v;
        }
        panic!("conn?");
    }
    pub fn get_code(&self) -> i32 {
        self.inner.code
    }
    pub fn get_heads(&self) -> &Option<ruisutil::bytes::ByteBox> {
        &self.inner.heads
    }
    pub fn get_bodys(
        &self,
        ctx: Option<ruisutil::Context>,
    ) -> &Option<ruisutil::bytes::ByteBox> {
        if !self.inner.bodyok.load(Ordering::SeqCst) {
            if self.inner.bodylen > 0 {
                let ins = unsafe { self.inner.muts() };
                if let Some(conn) = &mut ins.conn {
                    let ctxs = match ctx {
                        None => ruisutil::Context::background(None),
                        Some(v) => v,
                    };
                    match ruisutil::tcp_read(&ctxs, conn, self.inner.bodylen) {
                        Ok(bts) => ins.bodys = Some(ruisutil::bytes::ByteBox::from(bts)),
                        Err(e) => println!("get_bodys tcp read err:{}", e),
                    }
                }
            }
            self.inner.bodyok.store(true, Ordering::SeqCst);
        }
        &self.inner.bodys
    }
    pub fn body_len(&self) -> usize {
        self.inner.bodylen
    }
    pub fn own_heads(&self) -> Option<ruisutil::bytes::ByteBox> {
        let ins = unsafe { self.inner.muts() };
        std::mem::replace(&mut ins.heads, None)
    }
    pub fn own_bodys(&self) -> Option<ruisutil::bytes::ByteBox> {
        let ins = unsafe { self.inner.muts() };
        std::mem::replace(&mut ins.bodys, None)
    }
}
