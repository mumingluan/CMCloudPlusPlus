//! Rebuildable final directory state, isolated by account and repository GUID.
//! No raw commit or manifest documents are stored in this database.
use axuanstor_sdk::{self as sdk, repository::{digest,DirectoryItem,ObjectRef,Repository,Snapshot,Version}};
use rusqlite::{params,Connection,OptionalExtension};
use serde::{Serialize,Deserialize};
use std::{collections::{BTreeMap,BTreeSet},path::{Path,PathBuf},time::Duration};
use std::io::Write;
use rand::RngCore;
use zeroize::Zeroizing;

type Result<T> = std::result::Result<T,String>;
const FORMAT:u32=2;
const MAX_DIRECTORY_BYTES:usize=64<<20;

#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {format:u32,repository:String,marker:ObjectRef,inventory:String,observation_digest:String,heads:Vec<ObjectRef>,#[serde(default)]semantic:String,#[serde(default)]auth:Option<Vec<u8>>}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryState {
    pub directory:String,
    pub items:Vec<DirectoryItem>,
    pub versions:BTreeMap<String,Vec<Version>>,
    pub conflicts:Vec<String>,
    pub branches:Vec<String>,
    #[serde(default)]pub semantic:String,
    #[serde(default)]pub complete:bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdk::repository::{Store,Change,Entry};
    use std::{sync::{Arc,Mutex},io::{Read,Cursor}};
    #[derive(Default)]struct Memory(Mutex<BTreeMap<String,(ObjectRef,Vec<u8>)>>);
    impl Store for Memory{
        fn cipher_entropy(&self,_:&str,_:&str)->sdk::Result<Option<sdk::repository::CipherEntropy>>{Ok(None)}
        fn list(&self,dir:&str)->sdk::Result<Vec<ObjectRef>>{Ok(self.0.lock().unwrap().values().filter(|(r,_)|r.key.rsplit_once('/').map(|x|x.0).unwrap_or("")==dir).map(|v|v.0.clone()).collect())}
        fn stat(&self,r:&ObjectRef)->sdk::Result<ObjectRef>{self.0.lock().unwrap().get(&r.key).map(|x|x.0.clone()).ok_or(sdk::Error::NotFound)}
        fn open(&self,r:&ObjectRef)->sdk::Result<Box<dyn Read+Send>>{Ok(Box::new(Cursor::new(self.0.lock().unwrap().get(&r.key).ok_or(sdk::Error::NotFound)?.1.clone())))}
        fn put_immutable(&self,key:&str,size:u64,hash:&str,reader:&mut dyn Read)->sdk::Result<ObjectRef>{let mut bytes=vec![];reader.read_to_end(&mut bytes)?;assert_eq!(bytes.len()as u64,size);assert_eq!(digest(&bytes),hash);let r=ObjectRef{key:key.into(),id:digest(key.as_bytes())[..32].into(),size,digest:hash.into(),is_dir:false};self.0.lock().unwrap().insert(key.into(),(r.clone(),bytes));Ok(r)}
    }
    fn repository(encrypted:bool)->Repository{Repository::create(Arc::new(Memory::default()),encrypted,None,encrypted.then_some([42;32]).as_ref()).unwrap()}
    fn snapshot(repo:&mut Repository)->Snapshot{
        let basis=repo.refresh().unwrap();
        repo.commit(&basis,&"e".repeat(32),vec![Change{path:"folder/child".into(),entry:Some(Entry::directory())},Change{path:"deleted/deep/item".into(),entry:None}]).unwrap();
        repo.refresh().unwrap()
    }
    #[test]fn final_state_reopens_by_guid_and_keeps_deleted_subtree_versions(){
        let root=tempfile::tempdir().unwrap();let mut repo=repository(false);let view=snapshot(&mut repo);let observation=repo.observed_state().unwrap();
        {let mut index=DirectoryIndex::open(root.path(),&repo).unwrap();index.replace(&repo,&view,&digest(b"inventory-a"),&observation).unwrap();assert!(index.matches(&digest(b"inventory-a"),&observation).unwrap());assert!(!index.matches(&digest(b"inventory-b"),&observation).unwrap());assert!(!index.matches(&digest(b"inventory-a"),b"foreign").unwrap());}
        let index=DirectoryIndex::open(root.path(),&repo).unwrap();assert!(index.path().file_stem().unwrap().to_string_lossy()==repo.descriptor.id);
        let (_,root_items)=index.read(&repo,"").unwrap().unwrap();assert_eq!(root_items.iter().map(|i|i.name.as_str()).collect::<Vec<_>>(),vec!["folder"]);
        let restored=index.snapshot(&repo).unwrap();assert_eq!(restored.versions,view.versions);assert_eq!(restored.heads,view.heads);assert_eq!(restored.conflicts,view.conflicts);
    }
    #[test]fn interrupted_replace_keeps_the_previous_complete_state(){
        let root=tempfile::tempdir().unwrap();let mut repo=repository(false);let view=snapshot(&mut repo);let obs=repo.observed_state().unwrap();let mut index=DirectoryIndex::open(root.path(),&repo).unwrap();index.replace(&repo,&view,&digest(b"before"),&obs).unwrap();
        let key=repo.local_state_key("folder").unwrap();index.db.execute_batch(&format!("CREATE TRIGGER interrupted BEFORE INSERT ON directories WHEN NEW.directory_key='{key}' BEGIN SELECT RAISE(ABORT,'test interruption');END;")).unwrap();
        assert!(index.replace(&repo,&view,&digest(b"after"),&obs).is_err());assert!(index.matches(&digest(b"before"),&obs).unwrap());assert_eq!(index.snapshot(&repo).unwrap().versions,view.versions);
    }
    #[test]fn encrypted_cache_hides_names_and_rejects_swapped_rows(){
        let root=tempfile::tempdir().unwrap();let mut repo=repository(true);let view=snapshot(&mut repo);let obs=repo.observed_state().unwrap();let mut index=DirectoryIndex::open(root.path(),&repo).unwrap();index.replace(&repo,&view,&digest(b"encrypted"),&obs).unwrap();
        assert_eq!(index.snapshot(&repo).unwrap().versions,view.versions);
        for p in std::fs::read_dir(root.path()).unwrap(){let p=p.unwrap().path();if p.is_file()&&p.extension().and_then(|p|p.to_str())!=Some("lock"){let bytes=std::fs::read(p).unwrap();assert!(!bytes.windows(b"deleted/deep/item".len()).any(|w|w==b"deleted/deep/item"));}}
        let root_key=repo.local_state_key("").unwrap();let child_key=repo.local_state_key("folder").unwrap();index.db.execute("UPDATE directories SET data=(SELECT data FROM directories WHERE directory_key=?2),sha256=(SELECT sha256 FROM directories WHERE directory_key=?2) WHERE directory_key=?1",params![root_key,child_key]).unwrap();
        assert!(index.read(&repo,"").is_err());
    }
    #[test]fn malformed_sqlite_rebuilds_only_the_derived_guid_index(){
        let root=tempfile::tempdir().unwrap();let mut repo=repository(false);let view=snapshot(&mut repo);let observation=repo.observed_state().unwrap();
        let path={let mut index=DirectoryIndex::open(root.path(),&repo).unwrap();index.replace(&repo,&view,&digest(b"before"),&observation).unwrap();index.path().to_path_buf()};
        let original=root.path().join("original-session.json");sdk::local_state::write(&original,b"retained original session").unwrap();
        sdk::local_state::write(&path,b"not a SQLite database").unwrap();
        let mut rebuilt=DirectoryIndex::open(root.path(),&repo).unwrap();assert!(rebuilt.read(&repo,"").unwrap().is_none());rebuilt.replace(&repo,&view,&digest(b"after"),&observation).unwrap();assert_eq!(rebuilt.snapshot(&repo).unwrap().versions,view.versions);assert_eq!(sdk::local_state::read(&original,128).unwrap(),b"retained original session");
    }
    #[test]fn partial_directory_rows_are_persistent_but_never_claim_a_complete_snapshot(){
        let root=tempfile::tempdir().unwrap();let mut repo=repository(false);let view=snapshot(&mut repo);let observation=repo.observed_state().unwrap();let items=view.list_directory("").unwrap();let semantic=view.semantic_revision().unwrap();
        let partial=sdk::repository::DirectoryView{snapshot:view.clone(),items,semantic:semantic.clone(),used_compact:true};
        {let mut index=DirectoryIndex::open(root.path(),&repo).unwrap();index.store_directory(&repo,&partial,"","frontier",&observation).unwrap();assert_eq!(index.revision().unwrap(),semantic);assert!(index.snapshot(&repo).is_err());}
        let index=DirectoryIndex::open(root.path(),&repo).unwrap();assert!(index.matches("frontier",&observation).unwrap());assert_eq!(index.read(&repo,"").unwrap().unwrap().1.len(),1);
    }
    #[test]fn forged_plain_rows_with_rewritten_checksums_cannot_authorize_partial_compaction(){
        let root=tempfile::tempdir().unwrap();let mut repo=repository(false);let view=snapshot(&mut repo);let observed=repo.observed_state().unwrap();let mut index=DirectoryIndex::open(root.path(),&repo).unwrap();index.replace(&repo,&view,&digest(b"inventory"),&observed).unwrap();
        let mut row=index.decoded(&repo,"folder").unwrap().unwrap();row.versions.clear();row.items.clear();let stored=repo.seal_local_state("folder",&serde_json::to_vec(&row).unwrap()).unwrap();
        index.db.execute("UPDATE directories SET data=?1,sha256=?2 WHERE directory_key=?3",params![&stored,digest(&stored),repo.local_state_key("folder").unwrap()]).unwrap();assert!(index.snapshot(&repo).is_err());
        let mut identity=index.identity().unwrap().unwrap();identity.semantic=digest(b"forged root");index.db.execute("UPDATE identity SET data=?1 WHERE singleton=1",[serde_json::to_vec(&identity).unwrap()]).unwrap();assert!(index.verified_snapshot(&repo).is_err());
    }
}
pub struct DirectoryIndex {db:Connection,#[cfg(test)]path:PathBuf,repository:String,marker:ObjectRef,auth_key:Zeroizing<[u8;32]>,_lock:std::fs::File}
fn sqlite(_:rusqlite::Error)->String{"本机对象库目录索引暂时不可用".into()}
fn sdk_error(_:sdk::Error)->String{"本机目录索引身份或加密校验未通过".into()}
impl DirectoryIndex {
    fn connect(path:&Path)->std::result::Result<Connection,rusqlite::Error>{
        let db=Connection::open(path)?;db.busy_timeout(Duration::from_secs(3))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA trusted_schema=OFF;
            CREATE TABLE IF NOT EXISTS identity (singleton INTEGER PRIMARY KEY CHECK(singleton=1),data BLOB NOT NULL);
            CREATE TABLE IF NOT EXISTS directories (directory_key TEXT PRIMARY KEY,data BLOB NOT NULL,sha256 TEXT NOT NULL);")?;
        Ok(db)
    }
    pub fn open(directory:&Path,repository:&Repository)->Result<Self>{
        sdk::local_state::directory(directory).map_err(sdk_error)?;
        let auth=directory.join(".snapshot-auth.key");
        if !auth.try_exists().map_err(|_|"本机缓存验证密钥不可用")?{
            match sdk::local_state::create(&auth){
                Ok(mut file)=>{let mut key=Zeroizing::new([0u8;32]);rand::rngs::OsRng.fill_bytes(&mut *key);file.write_all(&*key).map_err(|_|"本机缓存验证密钥写入失败")?;file.sync_all().map_err(|_|"本机缓存验证密钥保存失败")?;},
                Err(sdk::Error::Io(e))if e.kind()==std::io::ErrorKind::AlreadyExists=>{},Err(e)=>return Err(sdk_error(e)),
            }
        }
        let raw=Zeroizing::new(sdk::local_state::read(&auth,32).map_err(sdk_error)?);let value:&[u8;32]=raw.as_slice().try_into().map_err(|_|"本机缓存验证密钥无效")?;let mut auth_key=Zeroizing::new([0u8;32]);auth_key.copy_from_slice(value);
        let marker=repository.marker_reference().map_err(sdk_error)?;
        let path=directory.join(format!("{}.sqlite3",repository.descriptor.id));
        let lock=sdk::local_state::lock(&path.with_extension("index.lock")).map_err(sdk_error)?;
        if !path.try_exists().map_err(|_|"本机目录索引路径不可用")?{
            match sdk::local_state::create(&path){Ok(f)=>drop(f),Err(sdk::Error::Io(e))if e.kind()==std::io::ErrorKind::AlreadyExists=>{},Err(e)=>return Err(sdk_error(e))}
        }
        // Validate ownership/reparse points before SQLite obtains the file.
        drop(sdk::local_state::open(&path,true).map_err(sdk_error)?);
        let db=match Self::connect(&path){Ok(db)=>db,Err(rusqlite::Error::SqliteFailure(code,_))if matches!(code.code,rusqlite::ErrorCode::DatabaseCorrupt|rusqlite::ErrorCode::NotADatabase)=>{
            // This GUID file is derived directory state. Durable observations,
            // provider upload journals and user exports are separate paths.
            for suffix in["-wal","-shm"]{let sidecar=PathBuf::from(format!("{}{suffix}",path.display()));if sidecar.try_exists().map_err(|_|"目录索引旁文件不可用")?{drop(sdk::local_state::open(&sidecar,true).map_err(sdk_error)?);std::fs::remove_file(sidecar).map_err(|_|"损坏的目录索引正在使用")?;}}
            let file=sdk::local_state::open(&path,true).map_err(sdk_error)?;file.set_len(0).map_err(|_|"损坏的目录索引无法重建")?;file.sync_all().map_err(|_|"目录索引重建未完成")?;
            Self::connect(&path).map_err(sqlite)?
        },Err(e)=>return Err(sqlite(e))};
        Ok(Self{db,#[cfg(test)]path,repository:repository.descriptor.id.clone(),marker,auth_key,_lock:lock})
    }
    fn identity(&self)->Result<Option<Identity>>{
        let raw:Option<Vec<u8>>=self.db.query_row("SELECT data FROM identity WHERE singleton=1",[],|r|r.get(0)).optional().map_err(sqlite)?;
        let Some(raw)=raw else{return Ok(None)};
        let value:Identity=serde_json::from_slice(&raw).map_err(|_|"本机目录索引描述已损坏")?;
        if value.format!=FORMAT||value.repository!=self.repository||value.marker!=self.marker{return Err("本机目录索引与当前对象库身份不一致".into())}
        Ok(Some(value))
    }
    pub fn matches(&self,inventory:&str,observation:&[u8])->Result<bool>{
        Ok(self.identity()?.is_some_and(|i|i.inventory==inventory&&i.observation_digest==digest(observation)))
    }
    pub fn has_directory(&self,repo:&Repository,directory:&str)->bool{
        let Ok(key)=repo.local_state_key(directory)else{return false};
        self.db.query_row("SELECT 1 FROM directories WHERE directory_key=?1",[key],|_|Ok(())).optional().ok().flatten().is_some()
    }
    fn decoded(&self,repo:&Repository,directory:&str)->Result<Option<DirectoryState>>{
        let key=repo.local_state_key(directory).map_err(sdk_error)?;
        let row:Option<(Vec<u8>,String)>=self.db.query_row("SELECT data,sha256 FROM directories WHERE directory_key=?1",[key],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(sqlite)?;
        let Some((stored,hash))=row else{return Ok(None)};
        if stored.len()>MAX_DIRECTORY_BYTES||digest(&stored)!=hash{return Err("本机目录索引内容已损坏".into())}
        let bytes=repo.open_local_state(directory,&stored).map_err(sdk_error)?;
        let state:DirectoryState=serde_json::from_slice(&bytes).map_err(|_|"本机目录索引内容已损坏")?;
        if state.directory!=directory{return Err("本机目录索引路径校验未通过".into())}
        Ok(Some(state))
    }
    pub fn read(&self,repo:&Repository,directory:&str)->Result<Option<(Snapshot,Vec<DirectoryItem>)>>{
        let Some(identity)=self.identity()? else{return Ok(None)};
        Ok(self.decoded(repo,directory)?.filter(|state|state.semantic==identity.semantic).map(|state|(Snapshot{repository:self.repository.clone(),heads:identity.heads,versions:state.versions,conflicts:state.conflicts},state.items)))
    }
    pub fn revision(&self)->Result<String>{Ok(self.identity()?.ok_or("本机目录索引尚未建立")?.semantic)}
    pub fn store_directory(&mut self,repo:&Repository,view:&sdk::repository::DirectoryView,directory:&str,inventory:&str,observation:&[u8])->Result<()> {
        if repo.marker_reference().map_err(sdk_error)?!=self.marker||view.snapshot.repository!=self.repository{return Err("对象库目录索引身份发生变化".into())}
        let local=DirectoryState{directory:directory.into(),items:view.items.clone(),versions:view.snapshot.versions.iter().filter(|(p,_)|p.rsplit_once('/').map(|p|p.0).unwrap_or("")==directory).map(|(p,v)|(p.clone(),v.clone())).collect(),conflicts:view.snapshot.conflicts.clone(),branches:vec![],semantic:view.semantic.clone(),complete:false};
        let bytes=serde_json::to_vec(&local).map_err(|_|"目录索引内容编码失败")?;
        if bytes.len()>MAX_DIRECTORY_BYTES-4096{return Err("单个目录超出本机索引分片上限".into())};
        let stored=repo.seal_local_state(directory,&bytes).map_err(sdk_error)?;
        let identity=serde_json::to_vec(&Identity{format:FORMAT,repository:self.repository.clone(),marker:self.marker.clone(),inventory:inventory.into(),observation_digest:digest(observation),heads:view.snapshot.heads.clone(),semantic:view.semantic.clone(),auth:None}).map_err(|_|"目录索引版本编码失败")?;
        let tx=self.db.transaction().map_err(sqlite)?;
        tx.execute("INSERT OR REPLACE INTO directories(directory_key,data,sha256) VALUES(?1,?2,?3)",params![repo.local_state_key(directory).map_err(sdk_error)?,&stored,digest(&stored)]).map_err(sqlite)?;
        tx.execute("INSERT OR REPLACE INTO identity(singleton,data) VALUES(1,?1)",[identity]).map_err(sqlite)?;tx.commit().map_err(sqlite)?;Ok(())
    }
    pub fn replace(&mut self,repo:&Repository,view:&Snapshot,inventory:&str,observation:&[u8])->Result<()> {
        if repo.marker_reference().map_err(sdk_error)?!=self.marker||view.repository!=self.repository{return Err("对象库目录索引身份发生变化".into())}
        let mut directories:BTreeMap<String,BTreeMap<String,Vec<Version>>>=BTreeMap::from([(String::new(),BTreeMap::new())]);
        for (path,versions)in &view.versions{
            let mut parent=path.rsplit_once('/').map(|x|x.0).unwrap_or("");
            directories.entry(parent.into()).or_default().insert(path.clone(),versions.clone());
            while !parent.is_empty(){directories.entry(parent.into()).or_default();parent=parent.rsplit_once('/').map(|x|x.0).unwrap_or("");}
            if versions.iter().any(|v|v.entry.as_ref().is_some_and(|e|e.kind=="directory")){directories.entry(path.clone()).or_default();}
        }
        let mut branches:BTreeMap<String,Vec<String>>=BTreeMap::new();
        for path in directories.keys().filter(|p|!p.is_empty()){
            branches.entry(path.rsplit_once('/').map(|x|x.0).unwrap_or("").into()).or_default().push(path.clone());
        }
        let semantic=view.semantic_revision().map_err(sdk_error)?;
        let auth=repo.seal_snapshot_cache(view,&self.auth_key,inventory,observation).map_err(sdk_error)?;
        let identity=serde_json::to_vec(&Identity{format:FORMAT,repository:self.repository.clone(),marker:self.marker.clone(),inventory:inventory.into(),observation_digest:digest(observation),heads:view.heads.clone(),semantic:semantic.clone(),auth:Some(auth)}).map_err(|_|"目录索引版本编码失败")?;
        let tx=self.db.transaction().map_err(sqlite)?;
        tx.execute("DELETE FROM directories",[]).map_err(sqlite)?;
        {
            let mut insert=tx.prepare("INSERT INTO directories(directory_key,data,sha256) VALUES(?1,?2,?3)").map_err(sqlite)?;
            for (directory,versions)in directories{
                let conflicts=view.conflicts.iter().filter(|p|directory.is_empty()||directory==**p||directory.starts_with(&((*p).clone()+"/"))||p.rsplit_once('/').map(|x|x.0).unwrap_or("")==directory).cloned().collect();
                let bytes=serde_json::to_vec(&DirectoryState{items:view.list_directory(&directory).map_err(sdk_error)?,branches:branches.remove(&directory).unwrap_or_default(),directory:directory.clone(),versions,conflicts,semantic:semantic.clone(),complete:true}).map_err(|_|"目录索引内容编码失败")?;
                if bytes.len()>MAX_DIRECTORY_BYTES-4096{return Err("单个目录超出本机索引分片上限".into())}
                let stored=repo.seal_local_state(&directory,&bytes).map_err(sdk_error)?;
                insert.execute(params![repo.local_state_key(&directory).map_err(sdk_error)?,&stored,digest(&stored)]).map_err(sqlite)?;
            }
        }
        tx.execute("INSERT OR REPLACE INTO identity(singleton,data) VALUES(1,?1)",[identity]).map_err(sqlite)?;
        tx.commit().map_err(sqlite)?;
        Ok(())
    }
    pub fn snapshot(&self,repo:&Repository)->Result<Snapshot>{
        Ok(self.verified_snapshot(repo)?.into_snapshot())
    }
    pub fn verified_snapshot(&self,repo:&Repository)->Result<sdk::repository::VerifiedCachedSnapshot>{
        let identity=self.identity()?.ok_or("对象库目录索引尚未建立")?;
        let mut view=Snapshot{repository:self.repository.clone(),heads:identity.heads,versions:BTreeMap::new(),conflicts:vec![]};
        let mut pending=vec![String::new()];let mut visited=BTreeSet::new();let mut conflicts=BTreeSet::new();
        while let Some(directory)=pending.pop(){
            if !visited.insert(directory.clone()){continue}
            let local=self.decoded(repo,&directory)?.ok_or("对象库目录索引不完整")?;
            if !local.complete||local.semantic!=identity.semantic{return Err("本机索引仅缓存部分目录".into())}
            view.versions.extend(local.versions);conflicts.extend(local.conflicts);
            pending.extend(local.branches);
        }
        view.conflicts=conflicts.into_iter().collect();repo.verify_snapshot_cache(view,&self.auth_key,identity.auth.as_deref().ok_or("完整缓存尚未验证，请刷新对象库")?).map_err(sdk_error)
    }
    #[cfg(test)]
    pub fn path(&self)->&Path{&self.path}
}
