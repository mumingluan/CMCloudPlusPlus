//! NDJSON bridge for the size-based mirror script. Credentials stay in memory.
#![allow(dead_code)]
#[path = "../src/auth.rs"] mod auth;
#[path = "../src/cloud.rs"] mod cloud;

use cloud::{AccountConfig, CloudClient, CloudFile, UploadPlan};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{io::{BufRead, Write}, path::{Path, PathBuf}, time::{Duration, UNIX_EPOCH}};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

fn vault() -> Result<AccountConfig, String> {
    use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN}};
    let path = PathBuf::from(std::env::var("APPDATA").map_err(|e|e.to_string())?).join("io.cmcloud.plus/account.vault");
    let bytes = std::fs::read(path).map_err(|e|e.to_string())?;
    let input = CRYPT_INTEGER_BLOB { cbData:bytes.len() as u32, pbData:bytes.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB { cbData:0, pbData:std::ptr::null_mut() };
    if unsafe { CryptUnprotectData(&input,std::ptr::null_mut(),std::ptr::null(),std::ptr::null(),std::ptr::null(),CRYPTPROTECT_UI_FORBIDDEN,&mut output) } == 0 {
        return Err("无法用当前 Windows 用户解密 CMCloud++ 凭据".into());
    }
    let data = unsafe { std::slice::from_raw_parts(output.pbData,output.cbData as usize).to_vec() };
    unsafe { LocalFree(output.pbData as _); }
    let value: Value = serde_json::from_slice(&data).map_err(|_|"凭据格式无效")?;
    let chosen = if let Some(accounts) = value["accounts"].as_array() {
        accounts.iter().find(|a| a["id"]==value["activeId"]).or_else(|| if accounts.len()==1 {accounts.first()} else {None})
            .ok_or("请先在 CMCloud++ 选择需要同步的账号")?.clone()
    } else { value };
    let mut config: AccountConfig = serde_json::from_value(chosen).map_err(|_|"账号格式无效")?;
    config.migrate_saved_space();
    if config.space=="share" {return Err("分享空间不能镜像同步".into());}
    // Bound part retry time without changing the application's saved preference.
    config.custom_upload_part_size=16*1024*1024;
    Ok(config)
}

fn emit(value: Value) {
    let mut out=std::io::stdout().lock();
    let _=serde_json::to_writer(&mut out,&value);let _=writeln!(out);let _=out.flush();
}
fn stamp(metadata:&std::fs::Metadata)->Result<u64,String>{
    Ok(metadata.modified().map_err(|e|e.to_string())?.duration_since(UNIX_EPOCH).map_err(|e|e.to_string())?.as_nanos() as u64)
}
fn checkpoint(path:&Path,value:&UploadState)->Result<(),String>{
    let temporary=path.with_extension("json.tmp");
    let bytes=serde_json::to_vec(value).map_err(|e|e.to_string())?;
    let mut file=std::fs::File::create(&temporary).map_err(|e|e.to_string())?;
    file.write_all(&bytes).map_err(|e|e.to_string())?;file.sync_all().map_err(|e|e.to_string())?;drop(file);
    std::fs::rename(temporary,path).map_err(|e|e.to_string())
}
#[derive(Serialize,Deserialize)]
struct UploadState { path:String,size:u64,mtime:u64,hash:String,plan:UploadPlan,parts:u64,complete:bool }

async fn upload(client:&CloudClient,request:&Value,local_root:&Path,work:&Path)->Result<Value,String>{
    let path=PathBuf::from(request["path"].as_str().ok_or("缺少本地路径")?);
    let canonical=tokio::fs::canonicalize(&path).await.map_err(|e|e.to_string())?;
    if !canonical.starts_with(local_root) || canonical==local_root {return Err("上传来源超出指定本地目录".into());}
    let parent:CloudFile=serde_json::from_value(request["parent"].clone()).map_err(|e|e.to_string())?;
    let name=request["name"].as_str().ok_or("缺少目标名称")?;
    let size=request["size"].as_u64().ok_or("缺少文件大小")?;
    let mtime=request["mtime"].as_u64().ok_or("缺少文件时间")?;
    let meta=tokio::fs::metadata(&canonical).await.map_err(|e|e.to_string())?;
    if !meta.is_file() || meta.len()!=size || stamp(&meta)?!=mtime {return Err("本地文件在清点后发生变化，停止该文件同步".into());}
    let key=request["key"].as_str().ok_or("缺少断点标识")?;
    if key.len()!=64 || !key.bytes().all(|b|b.is_ascii_hexdigit()) {return Err("断点标识无效".into());}
    let journal=work.join(format!("upload-{key}.json"));
    let old=std::fs::read(&journal).ok().and_then(|bytes|serde_json::from_slice::<UploadState>(&bytes).ok());
    let mut state=if let Some(old)=old {
        if old.path!=canonical.to_string_lossy() || old.size!=size || old.mtime!=mtime || old.plan.name!=name {
            return Err("上传断点与当前源文件不一致".into());
        }
        old
    } else {
        emit(json!({"kind":"progress","phase":"hashing","bytes":0,"total":size}));
        let mut file=tokio::fs::File::open(&canonical).await.map_err(|e|e.to_string())?;
        let mut hash=Sha256::new();let mut buffer=vec![0;1024*1024];let mut read=0u64;
        loop {let count=file.read(&mut buffer).await.map_err(|e|e.to_string())?;if count==0{break;}hash.update(&buffer[..count]);read+=count as u64;}
        let after=file.metadata().await.map_err(|e|e.to_string())?;
        if read!=size || after.len()!=size || stamp(&after)?!=mtime {return Err("计算上传哈希时本地文件发生变化".into());}
        let hash=hex::encode_upper(hash.finalize());
        let mut plan=client.prepare_upload(&parent,name,size,&hash).await?;
        plan.parts.clear();
        let state=UploadState{path:canonical.to_string_lossy().into_owned(),size,mtime,hash,plan,parts:0,complete:false};
        checkpoint(&journal,&state)?;state
    };
    if !state.complete {
        let count=size.max(1).div_ceil(state.plan.part_size);
        if !state.plan.rapid && !state.plan.existing {
            for number in state.parts+1..=count {
                let mut last=String::new();let mut success=false;
                for attempt in 0..5 {
                    if attempt>0 {tokio::time::sleep(Duration::from_secs((attempt*2) as u64)).await;}
                    let result=async {
                        let current=tokio::fs::metadata(&canonical).await.map_err(|e|e.to_string())?;
                        if current.len()!=size || stamp(&current)?!=mtime{return Err("本地文件在上传中发生变化".to_owned());}
                        let parts=client.refresh_upload_parts(&state.plan,number,1).await?;
                        let part=parts.first().ok_or("云端未返回上传分片")?;
                        let expected=(size.saturating_sub((number-1)*state.plan.part_size)).min(state.plan.part_size);
                        if part.part_number!=number || part.offset!=(number-1)*state.plan.part_size || part.size!=expected{return Err("云端上传分片范围无效".to_owned());}
                        let mut file=tokio::fs::File::open(&canonical).await.map_err(|e|e.to_string())?;
                        file.seek(std::io::SeekFrom::Start(part.offset)).await.map_err(|e|e.to_string())?;
                        let stream=ReaderStream::with_capacity(file.take(part.size),256*1024);
                        tokio::time::timeout(Duration::from_secs(600),client.upload_part(part,reqwest::Body::wrap_stream(stream))).await.map_err(|_|"分片上传超时".to_owned())??;
                        Ok::<(),String>(())
                    }.await;
                    match result {Ok(())=>{success=true;break;},Err(error)=>last=error,}
                }
                if !success {return Err(last);}
                state.parts=number;checkpoint(&journal,&state)?;
                emit(json!({"kind":"progress","phase":"uploading","bytes":(number*state.plan.part_size).min(size),"total":size}));
            }
        }
        let current=tokio::fs::metadata(&canonical).await.map_err(|e|e.to_string())?;
        if current.len()!=size || stamp(&current)?!=mtime{return Err("文件在上传后发生变化，未提交完成".into());}
        client.complete_upload(&state.plan).await?;
        state.complete=true;checkpoint(&journal,&state)?;
    }
    Ok(json!({"id":state.plan.file_id,"size":size,"name":state.plan.name,"rapid":state.plan.rapid||state.plan.existing}))
}

#[tokio::main]
async fn main(){
    let result=run().await;if let Err(error)=result {emit(json!({"kind":"fatal","error":error}));std::process::exit(1);}
}
async fn run()->Result<(),String>{
    let args:Vec<_>=std::env::args().collect();
    let local_root=std::fs::canonicalize(args.get(1).ok_or("缺少本地根目录")?).map_err(|e|e.to_string())?;
    let work=std::fs::canonicalize(args.get(2).ok_or("缺少日志目录")?).map_err(|e|e.to_string())?;
    let client=CloudClient::new(vault()?)?;
    let config=client.initialize().await?;
    emit(json!({"kind":"ready","accountId":config.id,"space":config.space,"root":client.root().await}));
    for line in std::io::stdin().lock().lines(){
        let request:Value=serde_json::from_str(&line.map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        let result:Result<Value,String>=async {
            let object=|field:&str|serde_json::from_value::<CloudFile>(request[field].clone()).map_err(|e|e.to_string());
            match request["op"].as_str().unwrap_or("") {
                "list"=>Ok(serde_json::to_value(client.list_all(&object("parent")?).await?).map_err(|e|e.to_string())?),
                "mkdir"=>Ok(serde_json::to_value(client.create_directory(&object("parent")?,request["name"].as_str().ok_or("缺少目录名称")?).await?).map_err(|e|e.to_string())?),
                "remove"=>{client.remove(&object("file")?).await?;Ok(json!(true))},
                "rename"=>{client.rename(&object("file")?,request["name"].as_str().ok_or("缺少名称")?).await?;Ok(json!(true))},
                "upload"=>upload(&client,&request,&local_root,&work).await,
                _=>Err("未知同步操作".into()),
            }
        }.await;
        match result {Ok(value)=>emit(json!({"kind":"result","ok":true,"value":value})),Err(error)=>emit(json!({"kind":"result","ok":false,"error":error})),}
    }
    Ok(())
}
