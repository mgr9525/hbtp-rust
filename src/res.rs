use std::{collections::HashMap, io, mem, net::TcpStream, time::Duration};

use qstring::QString;

pub const MaxOther: u64 = 1024 * 1024 * 20; //20M
pub const MaxHeads: u64 = 1024 * 1024 * 100; //100M
pub const MaxBodys: u64 = 1024 * 1024 * 1024; //1G

pub fn ParseContext(ctx: &ruisutil::Context, mut conn: TcpStream) -> io::Result<Context> {
    let mut info = MsgInfo::new();
    let infoln = mem::size_of::<MsgInfo>();
    let ctx = ruisutil::Context::with_timeout(Some(ctx.clone()), Duration::from_secs(10));
    let bts = ruisutil::tcp_read(&ctx, &mut conn, infoln)?;
    ruisutil::byte2struct(&mut info, &bts[..])?;
    if info.version != 1 {
        return Err(ruisutil::ioerr("not found version!", None));
    }
    if (info.lenCmd + info.lenArg) as u64 > MaxOther {
        return Err(ruisutil::ioerr("bytes1 out limit!!", None));
    }
    if (info.lenHead) as u64 > MaxHeads {
        return Err(ruisutil::ioerr("bytes2 out limit!!", None));
    }
    if (info.lenBody) as u64 > MaxBodys {
        return Err(ruisutil::ioerr("bytes3 out limit!!", None));
    }
    let mut rt = Context::new(info.control);
    let lnsz = info.lenCmd as usize;
    if lnsz > 0 {
        let bts = ruisutil::tcp_read(&ctx, &mut conn, lnsz)?;
        rt.cmds = match std::str::from_utf8(&bts[..]) {
            Err(e) => return Err(ruisutil::ioerr("cmd err", None)),
            Ok(v) => String::from(v),
        };
    }
    let lnsz = info.lenArg as usize;
    if lnsz > 0 {
        let bts = ruisutil::tcp_read(&ctx, &mut conn, lnsz as usize)?;
        let args = match std::str::from_utf8(&bts[..]) {
            Err(e) => return Err(ruisutil::ioerr("args err", None)),
            Ok(v) => String::from(v),
        };
        rt.args = Some(QString::from(args.as_str()));
    }
    let ctx = ruisutil::Context::with_timeout(Some(ctx.clone()), Duration::from_secs(30));
    let lnsz = info.lenHead as usize;
    if lnsz > 0 {
        let bts = ruisutil::tcp_read(&ctx, &mut conn, lnsz as usize)?;
        rt.heads = Some(bts);
    }
    let ctx = ruisutil::Context::with_timeout(Some(ctx.clone()), Duration::from_secs(50));
    let lnsz = info.lenBody as usize;
    if lnsz > 0 {
        let bts = ruisutil::tcp_read(&ctx, &mut conn, lnsz as usize)?;
        rt.bodys = Some(bts);
    }
    rt.conn = Some(conn);
    Ok(rt)
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
    pub fn own_heads(&mut self) -> Option<Box<[u8]>> {
        std::mem::replace(&mut self.heads, None)
    }
    pub fn own_bodys(&mut self) -> Option<Box<[u8]>> {
        std::mem::replace(&mut self.bodys, None)
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
            None => return Err(ruisutil::ioerr("not found conn", None)),
        };
        if self.sended {
            return Err(ruisutil::ioerr("already responsed!", None));
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
        let bts = ruisutil::struct2byte(&res);
        let ctx = ruisutil::Context::with_timeout(None, Duration::from_secs(10));
        ruisutil::tcp_write(&ctx, conn, bts)?;
        if let Some(v) = hds {
            let ctx = ruisutil::Context::with_timeout(None, Duration::from_secs(20));
            ruisutil::tcp_write(&ctx, conn, v)?;
        }
        if let Some(v) = bds {
            let ctx = ruisutil::Context::with_timeout(None, Duration::from_secs(30));
            ruisutil::tcp_write(&ctx, conn, v)?;
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
pub struct MsgInfo {
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
pub struct ResInfoV1 {
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
