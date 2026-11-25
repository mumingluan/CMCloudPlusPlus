//! 1:1 OpenList (`drivers/crypt`) & rclone (`backend/crypt`) compatible cipher:
//! - Key derivation: scrypt (N=16384, r=8, p=1, 80 bytes) + rclone `obscure` (`___Obfuscated___`)
//! - Name modes: `standard` (AES-256-EME + PKCS#7 + base64/base32/base32768), `obfuscate`, `off`
//! - Content format: `RCLONE\x00\x00` + 24-byte nonce + 64 KiB XSalsa20-Poly1305 (`secretbox`) blocks
//! - Seekable random-access block & byte-range encryption/decryption without temporary files.

use aes::{
    cipher::{BlockDecrypt, BlockEncrypt, KeyInit as AesKeyInit, KeyIvInit, StreamCipher},
    Aes256, Block,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use crypto_secretbox::{
    aead::AeadInPlace, Key as SecretboxKey, Nonce as SecretboxNonce, Tag as SecretboxTag,
    XSalsa20Poly1305,
};
#[cfg(test)]
use rand::{rngs::OsRng, RngCore};
use scrypt::{scrypt, Params as ScryptParams};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::io::{self, Read};
use std::sync::OnceLock;
use zeroize::Zeroizing;

type Aes256Ctr = ctr::Ctr128BE<Aes256>;

pub const FILE_MAGIC: &[u8; 8] = b"RCLONE\x00\x00";
pub const FILE_NONCE_SIZE: usize = 24;
pub const FILE_HEADER_SIZE: u64 = 32;
pub const BLOCK_HEADER_SIZE: u64 = 16;
pub const BLOCK_DATA_SIZE: u64 = 65536;
pub const BLOCK_SIZE: u64 = BLOCK_HEADER_SIZE + BLOCK_DATA_SIZE; // 65552

const NAME_CIPHER_BLOCK_SIZE: usize = 16;
const MAX_CIPHERTEXT_NAME_BYTES: usize = 2048;
const OBFUSC_QUOTE: char = '!';

const DEFAULT_SALT: [u8; 16] = [
    0xA8, 0x0D, 0xF4, 0x3A, 0x8F, 0xBD, 0x03, 0x08, 0xA7, 0xCA, 0xB8, 0x3E, 0x58, 0x1F, 0x86, 0xB1,
];

const RCLONE_OBSCURE_KEY: [u8; 32] = [
    0x9c, 0x93, 0x5b, 0x48, 0x73, 0x0a, 0x55, 0x4d, 0x6b, 0xfd, 0x7c, 0x63, 0xc8, 0x86, 0xa9, 0x2b,
    0xd3, 0x90, 0x19, 0x8e, 0xb8, 0x12, 0x8a, 0xfb, 0xf4, 0xde, 0x16, 0x2b, 0x8b, 0x95, 0xf6, 0x38,
];

const BASE32768_SAFE_ALPHABET: &str = "ƀɀɠʀҠԀڀڠݠހ߀ကႠᄀᄠᅀᆀᇠሀሠበዠጠᎠᏀᐠᑀᑠᒀᒠᓀᓠᔀᔠᕀᕠᖀᖠᗀᗠᘀᘠᙀᚠᛀកᠠᡀᣀᦀ᧠ᨠᯀᰀᴀ⇠⋀⍀⍠⎀⎠⏀␀─┠╀╠▀■◀◠☀☠♀♠⚀⚠⛀⛠✀✠❀➀➠⠀⠠⡀⡠⢀⢠⣀⣠⤀⤠⥀⥠⦠⨠⩀⪀⪠⫠⬀⬠⭀ⰀⲀⲠⳀⴀⵀ⺠⻀㇀㐀㐠㑀㑠㒀㒠㓀㓠㔀㔠㕀㕠㖀㖠㗀㗠㘀㘠㙀㙠㚀㚠㛀㛠㜀㜠㝀㝠㞀㞠㟀㟠㠀㠠㡀㡠㢀㢠㣀㣠㤀㤠㥀㥠㦀㦠㧀㧠㨀㨠㩀㩠㪀㪠㫀㫠㬀㬠㭀㭠㮀㮠㯀㯠㰀㰠㱀㱠㲀㲠㳀㳠㴀㴠㵀㵠㶀㶠㷀㷠㸀㸠㹀㹠㺀㺠㻀㻠㼀㼠㽀㽠㾀㾠㿀㿠䀀䀠䁀䁠䂀䂠䃀䃠䄀䄠䅀䅠䆀䆠䇀䇠䈀䈠䉀䉠䊀䊠䋀䋠䌀䌠䍀䍠䎀䎠䏀䏠䐀䐠䑀䑠䒀䒠䓀䓠䔀䔠䕀䕠䖀䖠䗀䗠䘀䘠䙀䙠䚀䚠䛀䛠䜀䜠䝀䝠䞀䞠䟀䟠䠀䠠䡀䡠䢀䢠䣀䣠䤀䤠䥀䥠䦀䦠䧀䧠䨀䨠䩀䩠䪀䪠䫀䫠䬀䬠䭀䭠䮀䮠䯀䯠䰀䰠䱀䱠䲀䲠䳀䳠䴀䴠䵀䵠䶀䷀䷠一丠乀习亀亠什仠伀传佀你侀侠俀俠倀倠偀偠傀傠僀僠儀儠兀兠冀冠净几刀删剀剠劀加勀勠匀匠區占厀厠叀叠吀吠呀呠咀咠哀哠唀唠啀啠喀喠嗀嗠嘀嘠噀噠嚀嚠囀因圀圠址坠垀垠埀埠堀堠塀塠墀墠壀壠夀夠奀奠妀妠姀姠娀娠婀婠媀媠嫀嫠嬀嬠孀孠宀宠寀寠尀尠局屠岀岠峀峠崀崠嵀嵠嶀嶠巀巠帀帠幀幠庀庠廀廠开张彀彠往徠忀忠怀怠恀恠悀悠惀惠愀愠慀慠憀憠懀懠戀戠所扠技抠拀拠挀挠捀捠掀掠揀揠搀搠摀摠撀撠擀擠攀攠敀敠斀斠旀无昀映晀晠暀暠曀曠最朠杀杠枀枠柀柠栀栠桀桠梀梠检棠椀椠楀楠榀榠槀槠樀樠橀橠檀檠櫀櫠欀欠歀歠殀殠毀毠氀氠汀池沀沠泀泠洀洠浀浠涀涠淀淠渀渠湀湠満溠滀滠漀漠潀潠澀澠激濠瀀瀠灀灠炀炠烀烠焀焠煀煠熀熠燀燠爀爠牀牠犀犠狀狠猀猠獀獠玀玠珀珠琀琠瑀瑠璀璠瓀瓠甀甠畀畠疀疠痀痠瘀瘠癀癠皀皠盀盠眀眠着睠瞀瞠矀矠砀砠础硠碀碠磀磠礀礠祀祠禀禠秀秠稀稠穀穠窀窠竀章笀笠筀筠简箠節篠簀簠籀籠粀粠糀糠紀素絀絠綀綠緀締縀縠繀繠纀纠绀绠缀缠罀罠羀羠翀翠耀耠聀聠肀肠胀胠脀脠腀腠膀膠臀臠舀舠艀艠芀芠苀苠茀茠荀荠莀莠菀菠萀萠葀葠蒀蒠蓀蓠蔀蔠蕀蕠薀薠藀藠蘀蘠虀虠蚀蚠蛀蛠蜀蜠蝀蝠螀螠蟀蟠蠀蠠血衠袀袠裀裠褀褠襀襠覀覠觀觠言訠詀詠誀誠諀諠謀謠譀譠讀讠诀诠谀谠豀豠貀負賀賠贀贠赀赠趀趠跀跠踀踠蹀蹠躀躠軀軠輀輠轀轠辀辠迀迠退造遀遠邀邠郀郠鄀鄠酀酠醀醠釀釠鈀鈠鉀鉠銀銠鋀鋠錀錠鍀鍠鎀鎠鏀鏠鐀鐠鑀鑠钀钠铀铠销锠镀镠門閠闀闠阀阠陀陠隀隠雀雠需霠靀靠鞀鞠韀韠頀頠顀顠颀颠飀飠餀餠饀饠馀馠駀駠騀騠驀驠骀骠髀髠鬀鬠魀魠鮀鮠鯀鯠鰀鰠鱀鱠鲀鲠鳀鳠鴀鴠鵀鵠鶀鶠鷀鷠鸀鸠鹀鹠麀麠黀黠鼀鼠齀齠龀龠ꀀꀠꁀꁠꂀꂠꃀꃠꄀꄠꅀꅠꆀꆠꇀꇠꈀꈠꉀꉠꊀꊠꋀꋠꌀꌠꍀꍠꎀꎠꏀꏠꐀꐠꑀꑠ꒠ꔀꔠꕀꕠꖀꖠꗀꗠꙀꚠꛀ꜀꜠ꝀꞀꡀ";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NameEncryptionMode {
    Off,
    Standard,
    Obfuscate,
}

impl NameEncryptionMode {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" => Ok(Self::Off),
            "standard" => Ok(Self::Standard),
            "obfuscate" => Ok(Self::Obfuscate),
            other => Err(format!("不支持的文件名加密模式：{other}")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NameEncoding {
    Base64,
    Base32,
    Base32768,
}

impl NameEncoding {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "base64" => Ok(Self::Base64),
            "base32" => Ok(Self::Base32),
            "base32768" => Ok(Self::Base32768),
            other => Err(format!("不支持的文件名编码格式：{other}")),
        }
    }

    pub fn encode(&self, src: &[u8]) -> String {
        match self {
            Self::Base64 => URL_SAFE_NO_PAD.encode(src),
            Self::Base32 => base32_hex_lower_encode(src),
            Self::Base32768 => base32768_safe().encode_to_string(src),
        }
    }

    pub fn decode(&self, s: &str) -> Result<Vec<u8>, String> {
        match self {
            Self::Base64 => URL_SAFE_NO_PAD
                .decode(s)
                .map_err(|_| "不是有效的 base64 加密名称".into()),
            Self::Base32 => base32_hex_lower_decode(s),
            Self::Base32768 => base32768_safe().decode_string(s),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CryptFolderOptions {
    pub filename_encryption: String,
    pub directory_name_encryption: bool,
    pub filename_encoding: String,
    pub encrypted_suffix: String,
    #[serde(default = "default_true")]
    pub show_hidden: bool,
}

fn default_true() -> bool {
    true
}

impl Default for CryptFolderOptions {
    fn default() -> Self {
        // Matches OpenList `drivers/crypt/meta.go` options (standard/off/obfuscate, base64, .bin).
        Self {
            filename_encryption: "standard".into(),
            directory_name_encryption: true,
            filename_encoding: "base64".into(),
            encrypted_suffix: ".bin".into(),
            show_hidden: true,
        }
    }
}

impl CryptFolderOptions {
    pub fn normalized(&self) -> Result<(NameEncryptionMode, bool, NameEncoding, String, bool), String> {
        let mode = NameEncryptionMode::parse(&self.filename_encryption)?;
        let enc = NameEncoding::parse(&self.filename_encoding)?;
        let suffix_raw = self.encrypted_suffix.trim();
        let suffix = if suffix_raw.eq_ignore_ascii_case("none") || suffix_raw.is_empty() && mode != NameEncryptionMode::Off {
            if suffix_raw.eq_ignore_ascii_case("none") {
                String::new()
            } else {
                ".bin".into()
            }
        } else if suffix_raw.is_empty() {
            ".bin".into()
        } else if suffix_raw.starts_with('.') {
            suffix_raw.to_string()
        } else {
            format!(".{suffix_raw}")
        };
        if !suffix.is_empty() && (suffix.len() > 32 || suffix.contains(['/', '\\', '\0'])) {
            return Err("加密文件后缀无效".into());
        }
        Ok((mode, self.directory_name_encryption, enc, suffix, self.show_hidden))
    }
}

/// Reveal an OpenList `___Obfuscated___...` password/salt or return plaintext directly.
pub fn normalize_openlist_secret(input: &str) -> Result<Zeroizing<String>, String> {
    if let Some(rest) = input.strip_prefix("___Obfuscated___") {
        rclone_reveal(rest)
    } else {
        Ok(Zeroizing::new(input.to_string()))
    }
}

pub fn rclone_reveal(obscured: &str) -> Result<Zeroizing<String>, String> {
    let mut raw = Zeroizing::new(
        URL_SAFE_NO_PAD
            .decode(obscured)
            .map_err(|_| "混淆口令解码失败")?,
    );
    if raw.len() < 16 {
        return Err("混淆口令长度不足".into());
    }
    let mut iv = [0u8; 16];
    iv.copy_from_slice(&raw[..16]);
    let body = &mut raw[16..];
    let mut ctr = Aes256Ctr::new((&RCLONE_OBSCURE_KEY).into(), (&iv).into());
    ctr.apply_keystream(body);
    let plain = std::str::from_utf8(body)
        .map_err(|_| "混淆口令不是有效的 UTF-8 文本")?
        .to_string();
    Ok(Zeroizing::new(plain))
}

#[cfg(test)]
pub fn rclone_obscure(plain: &str) -> String {
    let mut iv = [0u8; 16];
    OsRng.fill_bytes(&mut iv);
    let mut out = Vec::with_capacity(16 + plain.len());
    out.extend_from_slice(&iv);
    out.extend_from_slice(plain.as_bytes());
    let mut ctr = Aes256Ctr::new((&RCLONE_OBSCURE_KEY).into(), (&iv).into());
    ctr.apply_keystream(&mut out[16..]);
    URL_SAFE_NO_PAD.encode(&out)
}

pub fn add_nonce_le(base: &[u8; FILE_NONCE_SIZE], mut delta: u64) -> [u8; FILE_NONCE_SIZE] {
    let mut out = *base;
    let mut carry = 0u16;
    for i in 0..FILE_NONCE_SIZE {
        let digit = (delta & 0xFF) as u16;
        delta >>= 8;
        let sum = out[i] as u16 + digit + carry;
        out[i] = (sum & 0xFF) as u8;
        carry = sum >> 8;
        if delta == 0 && carry == 0 {
            break;
        }
    }
    out
}

pub struct OpenListCipher {
    data_key: Zeroizing<[u8; 32]>,
    name_key: Zeroizing<[u8; 32]>,
    name_tweak: Zeroizing<[u8; 16]>,
    aes: Aes256,
    mode: NameEncryptionMode,
    dir_name_encrypt: bool,
    encoding: NameEncoding,
    encrypted_suffix: String,
    show_hidden: bool,
}

impl OpenListCipher {
    pub fn new(password: &str, salt: &str, options: &CryptFolderOptions) -> Result<Self, String> {
        let (mode, dir_name_encrypt, encoding, encrypted_suffix, show_hidden) =
            options.normalized()?;
        let plain_password = normalize_openlist_secret(password)?;
        let plain_salt = if salt.is_empty() {
            Zeroizing::new(String::new())
        } else {
            normalize_openlist_secret(salt)?
        };
        if plain_password.is_empty() {
            return Err("请输入加密文件夹密码".into());
        }

        let salt_bytes: &[u8] = if plain_salt.is_empty() {
            &DEFAULT_SALT
        } else {
            plain_salt.as_bytes()
        };

        let mut derived = Zeroizing::new([0u8; 80]);
        let params = ScryptParams::new(14, 8, 1, 32).map_err(|_| "scrypt 参数初始化失败")?;
        scrypt(plain_password.as_bytes(), salt_bytes, &params, &mut *derived)
            .map_err(|_| "密钥派生失败")?;

        let mut data_key = Zeroizing::new([0u8; 32]);
        let mut name_key = Zeroizing::new([0u8; 32]);
        let mut name_tweak = Zeroizing::new([0u8; 16]);
        data_key.copy_from_slice(&derived[0..32]);
        name_key.copy_from_slice(&derived[32..64]);
        name_tweak.copy_from_slice(&derived[64..80]);

        let aes = Aes256::new_from_slice(&*name_key).map_err(|_| "AES-256 密钥初始化失败")?;

        Ok(Self {
            data_key,
            name_key,
            name_tweak,
            aes,
            mode,
            dir_name_encrypt,
            encoding,
            encrypted_suffix,
            show_hidden,
        })
    }

    pub fn show_hidden(&self) -> bool {
        self.show_hidden
    }

    pub fn encrypted_size(&self, plain_size: u64) -> u64 {
        let blocks = plain_size / BLOCK_DATA_SIZE;
        let residue = plain_size % BLOCK_DATA_SIZE;
        let mut total = FILE_HEADER_SIZE + blocks * BLOCK_SIZE;
        if residue != 0 {
            total += BLOCK_HEADER_SIZE + residue;
        }
        total
    }

    pub fn decrypted_size(&self, enc_size: u64) -> Result<u64, String> {
        if enc_size < FILE_HEADER_SIZE {
            return Err("加密文件长度不足（小于 32 字节文件头）".into());
        }
        let body = enc_size - FILE_HEADER_SIZE;
        let blocks = body / BLOCK_SIZE;
        let mut residue = body % BLOCK_SIZE;
        if residue != 0 {
            if residue <= BLOCK_HEADER_SIZE {
                return Err("加密文件尾块头部不完整".into());
            }
            residue -= BLOCK_HEADER_SIZE;
        }
        Ok(blocks * BLOCK_DATA_SIZE + residue)
    }

    pub fn encrypt_file_name(&self, input: &str) -> String {
        if self.mode == NameEncryptionMode::Off {
            return format!("{input}{}", self.encrypted_suffix);
        }
        self.encrypt_path_internal(input, true)
    }

    pub fn decrypt_file_name(&self, input: &str) -> Result<String, String> {
        if self.mode == NameEncryptionMode::Off {
            if !input.ends_with(&self.encrypted_suffix) || input.len() <= self.encrypted_suffix.len() {
                return Err("不是受支持的加密文件后缀".into());
            }
            let decrypted = &input[..input.len() - self.encrypted_suffix.len()];
            if let Some((_, unversioned)) = rclone_version_remove(decrypted) {
                if unversioned.is_empty() {
                    return Err("不是有效的版本化加密文件名".into());
                }
            }
            return Ok(decrypted.to_string());
        }
        self.decrypt_path_internal(input, true)
    }

    pub fn encrypt_dir_name(&self, input: &str) -> String {
        if self.mode == NameEncryptionMode::Off || !self.dir_name_encrypt {
            return input.to_string();
        }
        self.encrypt_path_internal(input, false)
    }

    pub fn decrypt_dir_name(&self, input: &str) -> Result<String, String> {
        if self.mode == NameEncryptionMode::Off || !self.dir_name_encrypt {
            return Ok(input.to_string());
        }
        match self.decrypt_path_internal(input, false) {
            Ok(out) => Ok(out),
            Err(err) => {
                let base = input.rsplit('/').next().unwrap_or(input);
                if rclone_version_remove(base).is_some() {
                    if let Ok(legacy) = self.decrypt_path_internal(input, true) {
                        return Ok(legacy);
                    }
                }
                Err(err)
            }
        }
    }

    fn encrypt_path_internal(&self, input: &str, strip_version: bool) -> String {
        let mut segments: Vec<String> = input.split('/').map(str::to_string).collect();
        let last_idx = segments.len().saturating_sub(1);
        for (i, seg) in segments.iter_mut().enumerate() {
            if !self.dir_name_encrypt && i != last_idx {
                continue;
            }
            let mut extracted_ver = None;
            if strip_version && i == last_idx {
                if let Some((ver_str, stripped)) = rclone_version_remove(seg) {
                    if stripped != *seg {
                        *seg = stripped;
                        extracted_ver = Some(ver_str);
                    }
                }
            }
            *seg = match self.mode {
                NameEncryptionMode::Standard => self.encrypt_segment(seg),
                NameEncryptionMode::Obfuscate => self.obfuscate_segment(seg),
                NameEncryptionMode::Off => seg.clone(),
            };
            if let Some(ver_str) = extracted_ver {
                *seg = rclone_version_add(seg, &ver_str);
            }
        }
        segments.join("/")
    }

    fn decrypt_path_internal(&self, input: &str, strip_version: bool) -> Result<String, String> {
        let mut segments: Vec<String> = input.split('/').map(str::to_string).collect();
        let last_idx = segments.len().saturating_sub(1);
        for (i, seg) in segments.iter_mut().enumerate() {
            if !self.dir_name_encrypt && i != last_idx {
                continue;
            }
            let mut extracted_ver = None;
            if strip_version && i == last_idx {
                if let Some((ver_str, stripped)) = rclone_version_remove(seg) {
                    if stripped != *seg {
                        *seg = stripped;
                        extracted_ver = Some(ver_str);
                    }
                }
            }
            *seg = match self.mode {
                NameEncryptionMode::Standard => self.decrypt_segment(seg)?,
                NameEncryptionMode::Obfuscate => self.deobfuscate_segment(seg)?,
                NameEncryptionMode::Off => seg.clone(),
            };
            if let Some(ver_str) = extracted_ver {
                *seg = rclone_version_add(seg, &ver_str);
            }
        }
        Ok(segments.join("/"))
    }

    fn encrypt_segment(&self, plaintext: &str) -> String {
        if plaintext.is_empty() {
            return String::new();
        }
        let padded = pkcs7_pad(plaintext.as_bytes());
        let ciphertext = eme_transform(&self.aes, &self.name_tweak, &padded, true);
        self.encoding.encode(&ciphertext)
    }

    fn decrypt_segment(&self, ciphertext: &str) -> Result<String, String> {
        if ciphertext.is_empty() {
            return Ok(String::new());
        }
        let raw = self.encoding.decode(ciphertext)?;
        if raw.is_empty() || raw.len() > MAX_CIPHERTEXT_NAME_BYTES || raw.len() % NAME_CIPHER_BLOCK_SIZE != 0 {
            return Err("加密文件名块长度无效".into());
        }
        let padded = eme_transform(&self.aes, &self.name_tweak, &raw, false);
        let unpadded = pkcs7_unpad(&padded)?;
        String::from_utf8(unpadded).map_err(|_| "解密后的文件名不是有效的 UTF-8".into())
    }

    fn obfuscate_segment(&self, plaintext: &str) -> String {
        if plaintext.is_empty() {
            return String::new();
        }
        let mut dir: i64 = 0;
        for ch in plaintext.chars() {
            dir += ch as u32 as i64;
        }
        dir %= 256;
        let mut out = format!("{dir}.");
        for &b in self.name_key.iter() {
            dir += b as i64;
        }
        for ch in plaintext.chars() {
            let code = ch as u32 as i64;
            if ch == OBFUSC_QUOTE {
                out.push(OBFUSC_QUOTE);
                out.push(OBFUSC_QUOTE);
            } else if ('0'..='9').contains(&ch) {
                let thisdir = (dir % 9) + 1;
                let new_code = ('0' as i64) + (code - ('0' as i64) + thisdir) % 10;
                out.push(char::from_u32(new_code as u32).unwrap_or(ch));
            } else if ch.is_ascii_alphabetic() {
                let thisdir = (dir % 25) + 1;
                let mut pos = code - ('A' as i64);
                if pos >= 26 {
                    pos -= 6;
                }
                pos = (pos + thisdir) % 52;
                if pos >= 26 {
                    pos += 6;
                }
                out.push(char::from_u32(('A' as i64 + pos) as u32).unwrap_or(ch));
            } else if (0xA0..=0xFF).contains(&code) {
                let thisdir = (dir % 95) + 1;
                let new_code = 0xA0 + (code - 0xA0 + thisdir) % 96;
                out.push(char::from_u32(new_code as u32).unwrap_or(ch));
            } else if code >= 0x100 {
                let thisdir = (dir % 127) + 1;
                let base = code - (code % 256);
                let new_code = (base + (code - base + thisdir) % 256) as u32;
                if let Some(valid) = char::from_u32(new_code) {
                    out.push(valid);
                } else {
                    out.push(OBFUSC_QUOTE);
                    out.push(ch);
                }
            } else {
                out.push(ch);
            }
        }
        out
    }

    fn deobfuscate_segment(&self, ciphertext: &str) -> Result<String, String> {
        if ciphertext.is_empty() {
            return Ok(String::new());
        }
        let (before, after) = ciphertext
            .split_once('.')
            .ok_or_else(|| "不是有效的混淆文件名".to_string())?;
        if before == "!" {
            return Ok(after.to_string());
        }
        let mut dir: i64 = before
            .parse::<i64>()
            .map_err(|_| "不是有效的混淆文件名前缀".to_string())?;
        if dir < 0 {
            return Err("混淆文件名前缀无效".into());
        }
        for &b in self.name_key.iter() {
            dir += b as i64;
        }
        let mut out = String::new();
        let mut in_quote = false;
        for ch in after.chars() {
            let code = ch as u32 as i64;
            if in_quote {
                out.push(ch);
                in_quote = false;
            } else if ch == OBFUSC_QUOTE {
                in_quote = true;
            } else if ('0'..='9').contains(&ch) {
                let thisdir = (dir % 9) + 1;
                let mut new_code = ('0' as i64) + code - ('0' as i64) - thisdir;
                if new_code < ('0' as i64) {
                    new_code += 10;
                }
                out.push(char::from_u32(new_code as u32).ok_or("混淆数字还原失败")?);
            } else if ch.is_ascii_alphabetic() {
                let thisdir = (dir % 25) + 1;
                let mut pos = code - ('A' as i64);
                if pos >= 26 {
                    pos -= 6;
                }
                pos -= thisdir;
                if pos < 0 {
                    pos += 52;
                }
                if pos >= 26 {
                    pos += 6;
                }
                out.push(char::from_u32(('A' as i64 + pos) as u32).ok_or("混淆字母还原失败")?);
            } else if (0xA0..=0xFF).contains(&code) {
                let thisdir = (dir % 95) + 1;
                let mut new_code = 0xA0 + code - 0xA0 - thisdir;
                if new_code < 0xA0 {
                    new_code += 96;
                }
                out.push(char::from_u32(new_code as u32).ok_or("混淆拉丁字符还原失败")?);
            } else if code >= 0x100 {
                let thisdir = (dir % 127) + 1;
                let base = code - (code % 256);
                let mut new_code = base + (code - base - thisdir);
                if new_code < base {
                    new_code += 256;
                }
                out.push(char::from_u32(new_code as u32).ok_or("混淆 Unicode 还原失败")?);
            } else {
                out.push(ch);
            }
        }
        if in_quote {
            return Err("混淆转义末尾不完整".into());
        }
        Ok(out)
    }

    pub fn seal_block(&self, block_index: u64, initial_nonce: &[u8; FILE_NONCE_SIZE], plain: &[u8]) -> Result<Vec<u8>, String> {
        if plain.is_empty() || plain.len() as u64 > BLOCK_DATA_SIZE {
            return Err("待加密明文分块大小无效".into());
        }
        let nonce = add_nonce_le(initial_nonce, block_index);
        let secretbox = XSalsa20Poly1305::new(SecretboxKey::from_slice(&*self.data_key));
        let mut out = Vec::with_capacity(BLOCK_HEADER_SIZE as usize + plain.len());
        out.extend_from_slice(&[0u8; 16]);
        out.extend_from_slice(plain);
        let tag = secretbox
            .encrypt_in_place_detached(SecretboxNonce::from_slice(&nonce), b"", &mut out[16..])
            .map_err(|_| "加密数据块失败".to_string())?;
        out[..16].copy_from_slice(tag.as_slice());
        Ok(out)
    }

    pub fn open_block(&self, block_index: u64, initial_nonce: &[u8; FILE_NONCE_SIZE], enc_block: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
        if enc_block.len() as u64 <= BLOCK_HEADER_SIZE || enc_block.len() as u64 > BLOCK_SIZE {
            return Err("加密文件块长度不合法".into());
        }
        let nonce = add_nonce_le(initial_nonce, block_index);
        let secretbox = XSalsa20Poly1305::new(SecretboxKey::from_slice(&*self.data_key));
        let tag = SecretboxTag::from_slice(&enc_block[..16]);
        let mut plain = Zeroizing::new(enc_block[16..].to_vec());
        secretbox
            .decrypt_in_place_detached(SecretboxNonce::from_slice(&nonce), b"", &mut plain, tag)
            .map_err(|_| "文件解密认证未通过（密码/盐值不匹配或数据损坏）".to_string())?;
        Ok(plain)
    }

    pub fn make_header(initial_nonce: &[u8; FILE_NONCE_SIZE]) -> [u8; 32] {
        let mut header = [0u8; 32];
        header[..8].copy_from_slice(FILE_MAGIC);
        header[8..32].copy_from_slice(initial_nonce);
        header
    }

    pub fn parse_header(header: &[u8]) -> Result<[u8; FILE_NONCE_SIZE], String> {
        if header.len() < 32 {
            return Err("加密文件头部长度不足 32 字节".into());
        }
        if &header[..8] != FILE_MAGIC {
            return Err("文件不包含有效的 OpenList/rclone 加密标头 (RCLONE\\x00\\x00)".into());
        }
        let mut nonce = [0u8; FILE_NONCE_SIZE];
        nonce.copy_from_slice(&header[8..32]);
        Ok(nonce)
    }

    #[cfg(test)]
    pub fn encrypt_bytes(&self, plain: &[u8], fixed_nonce: Option<[u8; FILE_NONCE_SIZE]>) -> Result<Vec<u8>, String> {
        let nonce = fixed_nonce.unwrap_or_else(|| {
            let mut n = [0u8; FILE_NONCE_SIZE];
            OsRng.fill_bytes(&mut n);
            n
        });
        let mut out = Vec::with_capacity(self.encrypted_size(plain.len() as u64) as usize);
        out.extend_from_slice(&Self::make_header(&nonce));
        for (idx, chunk) in plain.chunks(BLOCK_DATA_SIZE as usize).enumerate() {
            let sealed = self.seal_block(idx as u64, &nonce, chunk)?;
            out.extend_from_slice(&sealed);
        }
        Ok(out)
    }

    #[cfg(test)]
    pub fn decrypt_bytes(&self, encrypted: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
        let plain_len = self.decrypted_size(encrypted.len() as u64)? as usize;
        let initial_nonce = Self::parse_header(&encrypted[..32])?;
        let mut out = Zeroizing::new(Vec::with_capacity(plain_len));
        for (idx, chunk) in encrypted[32..].chunks(BLOCK_SIZE as usize).enumerate() {
            let opened = self.open_block(idx as u64, &initial_nonce, chunk)?;
            out.extend_from_slice(&opened);
        }
        Ok(out)
    }
}

#[cfg(test)]
pub struct RcloneDecryptReader<R: Read> {
    inner: R,
    cipher: std::sync::Arc<OpenListCipher>,
    initial_nonce: Option<[u8; FILE_NONCE_SIZE]>,
    block_index: u64,
    buf: Zeroizing<Vec<u8>>,
    pos: usize,
    eof: bool,
}

#[cfg(test)]
impl<R: Read> RcloneDecryptReader<R> {
    pub fn new(inner: R, cipher: std::sync::Arc<OpenListCipher>) -> Self {
        Self {
            inner,
            cipher,
            initial_nonce: None,
            block_index: 0,
            buf: Zeroizing::new(Vec::new()),
            pos: 0,
            eof: false,
        }
    }
}

#[cfg(test)]
impl<R: Read> Read for RcloneDecryptReader<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() || self.eof && self.pos >= self.buf.len() {
            return Ok(0);
        }
        if self.initial_nonce.is_none() {
            let mut header = [0u8; 32];
            read_full_or_eof(&mut self.inner, &mut header, false)?;
            let nonce = OpenListCipher::parse_header(&header)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            self.initial_nonce = Some(nonce);
        }
        if self.pos >= self.buf.len() {
            let mut enc_buf = vec![0u8; BLOCK_SIZE as usize];
            let n = read_up_to_block(&mut self.inner, &mut enc_buf)?;
            if n == 0 {
                self.eof = true;
                return Ok(0);
            }
            let nonce = self.initial_nonce.as_ref().unwrap();
            let plain = self
                .cipher
                .open_block(self.block_index, nonce, &enc_buf[..n])
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            self.block_index += 1;
            self.buf = plain;
            self.pos = 0;
        }
        let avail = self.buf.len() - self.pos;
        let to_copy = out.len().min(avail);
        out[..to_copy].copy_from_slice(&self.buf[self.pos..self.pos + to_copy]);
        self.pos += to_copy;
        Ok(to_copy)
    }
}

#[cfg(test)]
fn read_full_or_eof<R: Read>(r: &mut R, buf: &mut [u8], allow_clean_eof: bool) -> io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        match r.read(&mut buf[total..])? {
            0 if total == 0 && allow_clean_eof => return Ok(0),
            0 => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "加密流意外截断")),
            n => total += n,
        }
    }
    Ok(total)
}

#[cfg(test)]
fn read_up_to_block<R: Read>(r: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        match r.read(&mut buf[total..])? {
            0 => break,
            n => total += n,
        }
    }
    Ok(total)
}

fn pkcs7_pad(src: &[u8]) -> Vec<u8> {
    let pad_len = NAME_CIPHER_BLOCK_SIZE - (src.len() % NAME_CIPHER_BLOCK_SIZE);
    let mut out = Vec::with_capacity(src.len() + pad_len);
    out.extend_from_slice(src);
    out.resize(src.len() + pad_len, pad_len as u8);
    out
}

fn pkcs7_unpad(src: &[u8]) -> Result<Vec<u8>, String> {
    if src.is_empty() || src.len() % NAME_CIPHER_BLOCK_SIZE != 0 {
        return Err("PKCS#7 填充块大小不合法".into());
    }
    let pad = *src.last().unwrap() as usize;
    if pad == 0 || pad > NAME_CIPHER_BLOCK_SIZE || pad > src.len() {
        return Err("PKCS#7 填充长度无效".into());
    }
    let mut diff = 0u8;
    for &b in &src[src.len() - pad..] {
        diff |= b ^ (pad as u8);
    }
    if diff != 0 {
        return Err("PKCS#7 填充字节不匹配".into());
    }
    Ok(src[..src.len() - pad].to_vec())
}

fn mult_by_two(block: &mut [u8; 16]) {
    let msb = block[15] >> 7;
    let mut tmp = [0u8; 16];
    tmp[0] = block[0].wrapping_mul(2) ^ if msb != 0 { 135 } else { 0 };
    for j in 1..16 {
        tmp[j] = block[j].wrapping_mul(2).wrapping_add(block[j - 1] >> 7);
    }
    *block = tmp;
}

fn xor_block(dst: &mut [u8; 16], src: &[u8; 16]) {
    for i in 0..16 {
        dst[i] ^= src[i];
    }
}

fn aes_transform_block(aes: &Aes256, block: &mut [u8; 16], encrypt: bool) {
    let b = Block::from_mut_slice(block);
    if encrypt {
        aes.encrypt_block(b);
    } else {
        aes.decrypt_block(b);
    }
}

/// Exact Halevi-Rogaway 2003 EME wide-block transform matching `github.com/rfjakob/eme`.
fn eme_transform(aes: &Aes256, tweak: &[u8; 16], input: &[u8], encrypt: bool) -> Vec<u8> {
    let m = input.len() / 16;
    let mut li = [0u8; 16];
    aes.encrypt_block(Block::from_mut_slice(&mut li));
    let mut l_table = vec![[0u8; 16]; m];
    for item in l_table.iter_mut() {
        mult_by_two(&mut li);
        *item = li;
    }

    let mut c = vec![[0u8; 16]; m];
    for j in 0..m {
        c[j].copy_from_slice(&input[j * 16..(j + 1) * 16]);
        xor_block(&mut c[j], &l_table[j]);
        aes_transform_block(aes, &mut c[j], encrypt);
    }

    let mut mp = c[0];
    xor_block(&mut mp, tweak);
    for block in c.iter().skip(1) {
        xor_block(&mut mp, block);
    }

    let mut mc = mp;
    aes_transform_block(aes, &mut mc, encrypt);

    let mut m_mask = mp;
    xor_block(&mut m_mask, &mc);
    for block in c.iter_mut().skip(1) {
        mult_by_two(&mut m_mask);
        xor_block(block, &m_mask);
    }

    let mut ccc1 = mc;
    xor_block(&mut ccc1, tweak);
    for block in c.iter().skip(1) {
        xor_block(&mut ccc1, block);
    }
    c[0] = ccc1;

    let mut out = Vec::with_capacity(input.len());
    for j in 0..m {
        aes_transform_block(aes, &mut c[j], encrypt);
        xor_block(&mut c[j], &l_table[j]);
        out.extend_from_slice(&c[j]);
    }
    out
}

const BASE32_HEX_LOWER: &[u8; 32] = b"0123456789abcdefghijklmnopqrstuv";

fn base32_hex_lower_encode(src: &[u8]) -> String {
    if src.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity((src.len() * 8 + 4) / 5);
    let mut buffer = 0u32;
    let mut bits_left = 0u8;
    for &byte in src {
        buffer = (buffer << 8) | (byte as u32);
        bits_left += 8;
        while bits_left >= 5 {
            bits_left -= 5;
            let idx = ((buffer >> bits_left) & 0x1F) as usize;
            out.push(BASE32_HEX_LOWER[idx] as char);
        }
    }
    if bits_left > 0 {
        let idx = ((buffer << (5 - bits_left)) & 0x1F) as usize;
        out.push(BASE32_HEX_LOWER[idx] as char);
    }
    out
}

fn base32_hex_lower_decode(s: &str) -> Result<Vec<u8>, String> {
    if s.is_empty() {
        return Ok(Vec::new());
    }
    if s.ends_with('=') {
        return Err("base32 名称不应包含填充符号".into());
    }
    let rem = s.len() % 8;
    if matches!(rem, 1 | 3 | 6) {
        return Err("base32 名称长度不合法".into());
    }
    let mut out = Vec::with_capacity(s.len() * 5 / 8);
    let mut buffer = 0u32;
    let mut bits_left = 0u8;
    for b in s.bytes() {
        let val = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'v' => b - b'a' + 10,
            b'A'..=b'V' => b - b'A' + 10,
            _ => return Err("包含非法的 base32hex 字符".into()),
        };
        buffer = (buffer << 5) | (val as u32);
        bits_left += 5;
        if bits_left >= 8 {
            bits_left -= 8;
            out.push(((buffer >> bits_left) & 0xFF) as u8);
        }
    }
    if bits_left > 0 && (buffer & ((1 << bits_left) - 1)) != 0 {
        return Err("base32 末尾非零填充位无效".into());
    }
    Ok(out)
}

struct Base32768Encoding {
    encode_a: [u16; 1024],
    encode_b: [u16; 4],
    decode_map: [u16; 2048],
    splitter: u16,
}

fn base32768_safe() -> &'static Base32768Encoding {
    static SAFE: OnceLock<Base32768Encoding> = OnceLock::new();
    SAFE.get_or_init(|| {
        let mut units: Vec<u16> = BASE32768_SAFE_ALPHABET
            .chars()
            .take(1028)
            .map(|r| r as u16)
            .collect();
        assert_eq!(units.len(), 1028);
        units.sort_unstable();
        let splitter = units[4];
        let mut encode_a = [0u16; 1024];
        let mut encode_b = [0u16; 4];
        encode_a.copy_from_slice(&units[4..1028]);
        encode_b.copy_from_slice(&units[0..4]);
        let mut decode_map = [0xFFFDu16; 2048];
        for (i, &u) in encode_a.iter().enumerate() {
            decode_map[(u >> 5) as usize] = (i as u16) << 5;
        }
        for (i, &u) in encode_b.iter().enumerate() {
            decode_map[(u >> 5) as usize] = (i as u16) << 5;
        }
        Base32768Encoding {
            encode_a,
            encode_b,
            decode_map,
            splitter,
        }
    })
}

impl Base32768Encoding {
    fn encode15(&self, src: u16) -> u16 {
        let src = src & 0x7FFF;
        self.encode_a[(src >> 5) as usize] | (src & 0x1F)
    }

    fn encode7(&self, src: u8) -> u16 {
        let src = (src & 0x7F) as u16;
        self.encode_b[(src >> 5) as usize] | (src & 0x1F)
    }

    fn decode_unit(&self, src: u16) -> Option<(u16, bool)> {
        let is_trailing = src < self.splitter;
        let dst = self.decode_map[(src >> 5) as usize];
        if dst == 0xFFFD {
            None
        } else {
            Some((dst | (src & 0x1F), is_trailing))
        }
    }

    fn encode_to_string(&self, mut src: &[u8]) -> String {
        let out_units_len = (8 * src.len() + 14) / 15;
        let mut dst = Vec::with_capacity(out_units_len);
        let mut left: u8 = 0;
        let mut leftn: u8 = 0;
        while !src.is_empty() {
            let mut chunk: u16 = (left as u16) << (15 - leftn);
            chunk |= (src[0] as u16) << (7 - leftn);
            if leftn < 7 && src.len() > 1 {
                chunk |= (src[1] as u16) >> (1 + leftn);
                left = src[1] & ((1u8 << (1 + leftn)) - 1);
                leftn += 1;
                src = &src[2..];
            } else {
                chunk |= (1u16 << (7 - leftn)) - 1;
                left = 0;
                leftn = 0;
                src = &src[1..];
            }
            dst.push(self.encode15(chunk));
        }
        if leftn > 0 {
            let mut rem = left << (7 - leftn);
            rem |= (1u8 << (7 - leftn)) - 1;
            dst.push(self.encode7(rem));
        }
        String::from_utf16(&dst).expect("valid BMP alphabet")
    }

    fn decode_string(&self, s: &str) -> Result<Vec<u8>, String> {
        let units: Vec<u16> = s
            .chars()
            .filter(|&c| c != '\r' && c != '\n')
            .encode_utf16_vec();
        let max_bytes = units.len() * 15 / 8 + 2;
        let mut out = Vec::with_capacity(max_bytes);
        let mut left: u8 = 0;
        let mut leftn: u8 = 0;
        for &chunk in &units {
            let (d, trailing) = self
                .decode_unit(chunk)
                .ok_or_else(|| "包含非法的 base32768 字符".to_string())?;
            if trailing {
                if leftn > 0 {
                    out.push((left << (8 - leftn)) | ((d >> (leftn - 1)) as u8));
                }
                return Ok(out);
            }
            if leftn > 0 {
                out.push((left << (8 - leftn)) | ((d >> (7 + leftn)) as u8));
                out.push((d >> (leftn - 1)) as u8);
                left = (d as u8) & ((1u8 << (leftn - 1)) - 1);
                leftn -= 1;
            } else {
                out.push((d >> 7) as u8);
                left = (d as u8) & 0x7F;
                leftn = 7;
            }
        }
        Ok(out)
    }
}

trait EncodeUtf16Vec: Iterator<Item = char> + Sized {
    fn encode_utf16_vec(self) -> Vec<u16> {
        let mut out = Vec::new();
        let mut buf = [0u16; 2];
        for c in self {
            out.extend_from_slice(c.encode_utf16(&mut buf));
        }
        out
    }
}
impl<I: Iterator<Item = char>> EncodeUtf16Vec for I {}

fn split_ext_rclone(file_name: &str) -> (&str, &str) {
    let slash = file_name.rfind('/').map(|i| i + 1).unwrap_or(0);
    let mut ext = "";
    for (idx, b) in file_name[slash..].bytes().enumerate().rev() {
        if b == b'.' {
            ext = &file_name[slash + idx..];
            break;
        }
    }
    let base = &file_name[..file_name.len() - ext.len()];
    if !ext.is_empty() && base.is_empty() {
        (ext, "")
    } else {
        (base, ext)
    }
}

fn rclone_version_remove(file_name: &str) -> Option<(String, String)> {
    const VER_LEN: usize = 23; // "-vYYYY-MM-DD-HHMMSS-mmm"
    let (base, ext) = split_ext_rclone(file_name);
    if base.len() < VER_LEN {
        return None;
    }
    let split_at = base.len() - VER_LEN;
    let bytes = &base.as_bytes()[split_at..];
    if bytes.len() != VER_LEN
        || &bytes[0..2] != b"-v"
        || bytes[6] != b'-'
        || bytes[9] != b'-'
        || bytes[12] != b'-'
        || bytes[19] != b'-'
    {
        return None;
    }
    let digits_at = |range: std::ops::Range<usize>| -> Option<u32> {
        std::str::from_utf8(&bytes[range]).ok()?.parse::<u32>().ok()
    };
    let year = digits_at(2..6)?;
    let month = digits_at(7..9)?;
    let day = digits_at(10..12)?;
    let hour = digits_at(13..15)?;
    let min = digits_at(15..17)?;
    let sec = digits_at(17..19)?;
    let _ms = digits_at(20..23)?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || min > 59
        || sec > 59
    {
        return None;
    }
    chrono::NaiveDate::from_ymd_opt(year as i32, month, day)?;
    // Since bytes[0] == b'-' (ASCII 0x2D), `split_at` is guaranteed to be a valid UTF-8 char boundary.
    let cand = &base[split_at..];
    let stripped = format!("{}{}", &base[..split_at], ext);
    Some((cand.to_string(), stripped))
}

fn rclone_version_add(file_name: &str, ver_str: &str) -> String {
    let (base, ext) = split_ext_rclone(file_name);
    format!("{base}{ver_str}{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obscure_and_reveal_roundtrip() {
        let plain = "OpenList-Secret-口令!@#123";
        let obs = rclone_obscure(plain);
        assert_eq!(&*rclone_reveal(&obs).unwrap(), plain);
        let openlist_wrapped = format!("___Obfuscated___{obs}");
        assert_eq!(&*normalize_openlist_secret(&openlist_wrapped).unwrap(), plain);
        assert_eq!(&*normalize_openlist_secret(plain).unwrap(), plain);
    }

    #[test]
    fn all_name_modes_and_encodings_roundtrip() {
        let samples = [
            "report.pdf",
            "中文文件夹/子目录/测试视频 4K 🚀.mp4",
            ".hidden",
            "archive-v2024-05-06-123456-789.tar.gz",
            "quote!mark!test.txt",
        ];
        for mode in ["standard", "obfuscate", "off"] {
            for dir_enc in [true, false] {
                for enc in ["base64", "base32", "base32768"] {
                    let opts = CryptFolderOptions {
                        filename_encryption: mode.into(),
                        directory_name_encryption: dir_enc,
                        filename_encoding: enc.into(),
                        encrypted_suffix: ".bin".into(),
                        show_hidden: true,
                    };
                    let cipher = OpenListCipher::new("my-pass", "my-salt", &opts).unwrap();
                    for s in samples {
                        let ef = cipher.encrypt_file_name(s);
                        let df = cipher.decrypt_file_name(&ef).unwrap();
                        assert_eq!(df, s, "mode={mode} dir={dir_enc} enc={enc}");
                    }
                    let dir = "照片备份/2026年/旅行!集";
                    let ed = cipher.encrypt_dir_name(dir);
                    let dd = cipher.decrypt_dir_name(&ed).unwrap();
                    assert_eq!(dd, dir);
                }
            }
        }
    }

    #[test]
    fn content_encryption_sizes_and_streaming_roundtrip() {
        let opts = CryptFolderOptions::default();
        let cipher = std::sync::Arc::new(OpenListCipher::new("p4ss", "", &opts).unwrap());
        for len in [0usize, 1, 15, 65535, 65536, 65537, 131072, 200_005] {
            let plain: Vec<u8> = (0..len).map(|i| ((i * 37 + 11) & 0xFF) as u8).collect();
            let enc = cipher.encrypt_bytes(&plain, None).unwrap();
            assert_eq!(enc.len() as u64, cipher.encrypted_size(len as u64));
            assert_eq!(cipher.decrypted_size(enc.len() as u64).unwrap(), len as u64);
            let dec = cipher.decrypt_bytes(&enc).unwrap();
            assert_eq!(&*dec, &plain[..]);

            let mut stream_dec = Vec::new();
            RcloneDecryptReader::new(&enc[..], cipher.clone())
                .read_to_end(&mut stream_dec)
                .unwrap();
            assert_eq!(stream_dec, plain);
        }
    }

    #[test]
    fn openlist_go_golden_vectors_match_1_to_1() {
        use sha2::{Digest, Sha256};

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct GoldenVector {
            password: String,
            salt: String,
            mode: String,
            dir_enc: bool,
            encoding: String,
            suffix: String,
            plain_file: String,
            enc_file: String,
            plain_dir: String,
            enc_dir: String,
            plain_len: usize,
            nonce_hex: String,
            enc_sha256: String,
        }
        let vectors: Vec<GoldenVector> =
            serde_json::from_str(include_str!("testdata_openlist_crypt_vectors.json")).unwrap();
        assert_eq!(vectors.len(), 18);
        for (i, v) in vectors.iter().enumerate() {
            let opts = CryptFolderOptions {
                filename_encryption: v.mode.clone(),
                directory_name_encryption: v.dir_enc,
                filename_encoding: v.encoding.clone(),
                encrypted_suffix: v.suffix.clone(),
                show_hidden: true,
            };
            let cipher = OpenListCipher::new(&v.password, &v.salt, &opts).unwrap();
            assert_eq!(
                cipher.encrypt_file_name(&v.plain_file),
                v.enc_file,
                "vector #{i} encrypt_file_name mismatch"
            );
            assert_eq!(
                cipher.decrypt_file_name(&v.enc_file).unwrap(),
                v.plain_file,
                "vector #{i} decrypt_file_name mismatch"
            );
            assert_eq!(
                cipher.encrypt_dir_name(&v.plain_dir),
                v.enc_dir,
                "vector #{i} encrypt_dir_name mismatch"
            );
            assert_eq!(
                cipher.decrypt_dir_name(&v.enc_dir).unwrap(),
                v.plain_dir,
                "vector #{i} decrypt_dir_name mismatch"
            );

            let plain: Vec<u8> = (0..v.plain_len)
                .map(|j| ((j * 73 + i * 19 + 5) & 0xFF) as u8)
                .collect();
            let nonce_vec = hex::decode(&v.nonce_hex).unwrap();
            let mut go_nonce = [0u8; FILE_NONCE_SIZE];
            go_nonce.copy_from_slice(&nonce_vec);
            let rust_enc = cipher.encrypt_bytes(&plain, Some(go_nonce)).unwrap();
            assert_eq!(
                hex::encode(Sha256::digest(&rust_enc)),
                v.enc_sha256,
                "vector #{i} Rust ciphertext did not match OpenList Go bit-for-bit!"
            );
            let dec = cipher.decrypt_bytes(&rust_enc).unwrap();
            assert_eq!(&*dec, &plain[..], "vector #{i} decrypt roundtrip failed");
        }
    }
}
