//! RSA/SHA-256 CMS signatures. Verification proves integrity, not CA trust.
use anyhow::{Context, Result, ensure};
use cms::{
    cert::{CertificateChoices, IssuerAndSerialNumber},
    content_info::{CmsVersion, ContentInfo},
    signed_data::*,
};
use der::{
    Any, Decode, Encode,
    asn1::{ObjectIdentifier, OctetString, SetOfVec},
};
use rsa::{
    RsaPrivateKey, RsaPublicKey,
    pkcs1v15::{Signature, SigningKey, VerifyingKey},
    pkcs8::{DecodePrivateKey, DecodePublicKey},
    signature::{SignatureEncoding, Signer, Verifier},
    traits::PublicKeyParts,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use spki::AlgorithmIdentifierOwned;

const DATA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.1");
const SIGNED: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.2");
const SHA256: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.16.840.1.101.3.4.2.1");
const RSA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1");
const RSA_SHA256: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.11");

pub struct Identity {
    key: RsaPrivateKey,
    chain: Vec<x509_cert::Certificate>,
}
impl Identity {
    pub fn from_pkcs12(bytes: &[u8], password: &str) -> Result<Self> {
        ensure!(bytes.len() <= 8 * 1024 * 1024, "证书包超过 8 MiB");
        let store = p12_keystore::KeyStore::from_pkcs12(bytes, password)
            .context("无法打开 P12/PFX，密码错误或证书包格式不支持")?;
        let keys: Vec<_> = store
            .entries()
            .filter_map(|(_, e)| {
                if let p12_keystore::KeyStoreEntry::PrivateKeyChain(k) = e {
                    Some(k)
                } else {
                    None
                }
            })
            .collect();
        ensure!(keys.len() == 1, "请使用只包含一个私钥的 P12/PFX 证书包");
        let k = keys[0];
        let key = RsaPrivateKey::from_pkcs8_der(k.key())
            .context("目前支持 RSA 证书，暂不支持 SM2/ECDSA 或硬件证书")?;
        ensure!(
            key.n().bits() >= 2048 && key.n().bits() <= 8192,
            "RSA 密钥需要 2048–8192 位"
        );
        let chain = k
            .chain()
            .iter()
            .map(|c| x509_cert::Certificate::from_der(c.as_der()).map_err(anyhow::Error::from))
            .collect::<Result<Vec<_>>>()?;
        ensure!(!chain.is_empty() && chain.len() <= 16, "证书链为空或过长");
        let public = RsaPublicKey::from_public_key_der(
            &chain[0].tbs_certificate.subject_public_key_info.to_der()?,
        )?;
        ensure!(public == RsaPublicKey::from(&key), "私钥与签名证书不匹配");
        if let Some((_, usage)) = chain[0]
            .tbs_certificate
            .get::<x509_cert::ext::pkix::KeyUsage>()?
        {
            ensure!(
                usage.digital_signature() || usage.non_repudiation(),
                "证书 KeyUsage 不允许文档签名"
            );
        }
        let now = std::time::SystemTime::now();
        let validity = &chain[0].tbs_certificate.validity;
        ensure!(
            now >= validity.not_before.to_system_time()
                && now <= validity.not_after.to_system_time(),
            "签名证书不在有效期内"
        );
        Ok(Self { key, chain })
    }
    pub fn subject(&self) -> String {
        self.chain[0].tbs_certificate.subject.to_string()
    }
    pub fn detached(&self, data: &[u8]) -> Result<Vec<u8>> {
        let signature = SigningKey::<Sha256>::new(self.key.clone())
            .sign(data)
            .to_vec();
        let signer = SignerInfo {
            version: CmsVersion::V1,
            sid: SignerIdentifier::IssuerAndSerialNumber(IssuerAndSerialNumber {
                issuer: self.chain[0].tbs_certificate.issuer.clone(),
                serial_number: self.chain[0].tbs_certificate.serial_number.clone(),
            }),
            digest_alg: algorithm(SHA256),
            signed_attrs: None,
            signature_algorithm: algorithm(RSA),
            signature: OctetString::new(signature)?,
            unsigned_attrs: None,
        };
        let signed = SignedData {
            version: CmsVersion::V1,
            digest_algorithms: SetOfVec::try_from(vec![algorithm(SHA256)])?,
            encap_content_info: EncapsulatedContentInfo {
                econtent_type: DATA,
                econtent: None,
            },
            certificates: Some(CertificateSet::try_from(
                self.chain
                    .iter()
                    .cloned()
                    .map(CertificateChoices::Certificate)
                    .collect::<Vec<_>>(),
            )?),
            crls: None,
            signer_infos: SignerInfos::try_from(vec![signer])?,
        };
        Ok(ContentInfo {
            content_type: SIGNED,
            content: Any::from_der(&signed.to_der()?)?,
        }
        .to_der()?)
    }
}
fn algorithm(oid: ObjectIdentifier) -> AlgorithmIdentifierOwned {
    AlgorithmIdentifierOwned {
        oid,
        parameters: Some(Any::null()),
    }
}
#[derive(Debug, Serialize)]
pub struct Verification {
    pub integrity_valid: bool,
    pub certificate_subject: String,
    pub certificate_issuer: String,
    pub certificate_sha256: String,
    pub certificate_currently_valid: bool,
    pub trust: String,
    pub signed_bytes: usize,
    pub file_fully_covered: bool,
}
pub fn verify_detached(data: &[u8], signature: &[u8]) -> Result<Verification> {
    ensure!(signature.len() <= 1024 * 1024, "签名文件过大");
    let info = ContentInfo::from_der(signature).context("不是 DER 编码的 CMS/P7S 签名")?;
    ensure!(info.content_type == SIGNED, "文件不是 CMS SignedData");
    let signed = SignedData::from_der(&info.content.to_der()?)?;
    ensure!(
        signed.encap_content_info.econtent_type == DATA
            && signed.encap_content_info.econtent.is_none(),
        "需要分离式 id-data 签名"
    );
    ensure!(signed.signer_infos.0.len() == 1, "目前验签仅支持单个签名者");
    let signer = signed.signer_infos.0.iter().next().context("没有签名者")?;
    ensure!(
        signer.digest_alg.oid == SHA256
            && (signer.signature_algorithm.oid == RSA
                || signer.signature_algorithm.oid == RSA_SHA256),
        "当前只支持 RSA/SHA-256 签名验证"
    );
    let cert = signed
        .certificates
        .as_ref()
        .context("签名没有证书")?
        .0
        .iter()
        .find_map(|c| {
            if let CertificateChoices::Certificate(cert) = c {
                match &signer.sid {
                    SignerIdentifier::IssuerAndSerialNumber(id)
                        if id.issuer == cert.tbs_certificate.issuer
                            && id.serial_number == cert.tbs_certificate.serial_number =>
                    {
                        Some(cert)
                    }
                    _ => None,
                }
            } else {
                None
            }
        })
        .context("找不到对应签名证书")?;
    let public =
        RsaPublicKey::from_public_key_der(&cert.tbs_certificate.subject_public_key_info.to_der()?)?;
    let message = if let Some(attrs) = &signer.signed_attrs {
        let digest_id = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.4");
        let type_id = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.3");
        let digest: Vec<_> = attrs.iter().filter(|a| a.oid == digest_id).collect();
        ensure!(
            digest.len() == 1 && digest[0].values.len() == 1,
            "缺少或重复 messageDigest 属性"
        );
        let stored = OctetString::from_der(&digest[0].values.iter().next().unwrap().to_der()?)?;
        ensure!(
            stored.as_bytes() == Sha256::digest(data).as_slice(),
            "签名摘要不匹配，文件已被修改"
        );
        let types: Vec<_> = attrs.iter().filter(|a| a.oid == type_id).collect();
        ensure!(
            types.len() == 1
                && types[0].values.len() == 1
                && ObjectIdentifier::from_der(&types[0].values.iter().next().unwrap().to_der()?)?
                    == DATA,
            "contentType 属性无效"
        );
        attrs.to_der()?
    } else {
        data.to_vec()
    };
    VerifyingKey::<Sha256>::new(public)
        .verify(&message, &Signature::try_from(signer.signature.as_bytes())?)
        .context("签名不匹配，文件已被修改或签名无效")?;
    let now = std::time::SystemTime::now();
    Ok(Verification {
        integrity_valid: true,
        certificate_subject: cert.tbs_certificate.subject.to_string(),
        certificate_issuer: cert.tbs_certificate.issuer.to_string(),
        certificate_sha256: hex::encode(Sha256::digest(cert.to_der()?)),
        certificate_currently_valid: now
            >= cert.tbs_certificate.validity.not_before.to_system_time()
            && now <= cert.tbs_certificate.validity.not_after.to_system_time(),
        trust: "未验证 CA 信任链、吊销状态或可信时间戳；摘要验签通过不等于身份可信认证".into(),
        signed_bytes: data.len(),
        file_fully_covered: true,
    })
}
/// Writes an interoperable adbe.pkcs7.detached PDF signature to a new PDF.
/// Refuses signed/encrypted PDFs rather than invalidating an existing signature.
pub fn sign_pdf(input: &[u8], identity: &Identity, reason: &str) -> Result<Vec<u8>> {
    use lopdf::{Dictionary, Object, StringFormat, dictionary};
    let mut pdf = lopdf::Document::load_mem(input)?;
    ensure!(!pdf.trailer.has(b"Encrypt"), "加密 PDF 暂不支持签名");
    ensure!(
        !pdf.objects.values().any(|o| o.as_dict().is_ok_and(|d| d
            .get(b"Type")
            .and_then(Object::as_name)
            .ok()
            == Some(b"Sig")
            && d.has(b"Contents"))),
        "PDF 已有数字签名，不能通过重写添加签名；请使用支持增量多重签名的软件"
    );
    let root = pdf.trailer.get(b"Root")?.as_reference()?;
    let reserved = 16384;
    let sig=pdf.add_object(dictionary!{"Type"=>"Sig","Filter"=>"Adobe.PPKLite","SubFilter"=>"adbe.pkcs7.detached","Name"=>pdf_string(&identity.subject()),"Reason"=>pdf_string(reason),"ByteRange"=>vec![0.into(),1111111111i64.into(),2222222222i64.into(),3333333333i64.into()],"Contents"=>Object::String(vec![0;reserved],StringFormat::Hexadecimal)});
    let field = pdf.add_object(
        dictionary! {"FT"=>"Sig","T"=>Object::string_literal("JianYueSignature"),"V"=>sig},
    );
    let existing = pdf
        .get_object(root)?
        .as_dict()?
        .get(b"AcroForm")
        .ok()
        .cloned();
    let (mut acro, acro_id) = match existing {
        Some(Object::Reference(id)) => (pdf.get_object(id)?.as_dict()?.clone(), Some(id)),
        Some(Object::Dictionary(d)) => (d, None),
        _ => (Dictionary::new(), None),
    };
    let mut fields = acro
        .get(b"Fields")
        .ok()
        .and_then(|o| pdf.dereference(o).ok())
        .and_then(|(_, o)| o.as_array().ok())
        .cloned()
        .unwrap_or_default();
    fields.push(field.into());
    acro.set("Fields", fields);
    acro.set("SigFlags", 3);
    let acro_id = if let Some(id) = acro_id {
        pdf.objects.insert(id, Object::Dictionary(acro));
        id
    } else {
        pdf.add_object(acro)
    };
    pdf.get_object_mut(root)?
        .as_dict_mut()?
        .set("AcroForm", acro_id);
    let mut bytes = vec![];
    pdf.save_to(&mut bytes)?;
    let object_marker = format!("{} {} obj", sig.0, sig.1);
    let begin = find(&bytes, object_marker.as_bytes()).context("找不到 PDF 签名对象")?;
    let end = begin + find(&bytes[begin..], b"endobj").context("签名对象未闭合")?;
    let contents = begin + find(&bytes[begin..end], b"/Contents").context("缺少签名占位区")?;
    let left = contents + find(&bytes[contents..end], b"<").context("缺少签名十六进制区")?;
    let right = left + find(&bytes[left..end], b">").context("签名区未闭合")? + 1;
    ensure!(right - left == reserved * 2 + 2, "签名占位区长度错误");
    let br = begin + find(&bytes[begin..end], b"/ByteRange").context("缺少 ByteRange")?;
    let a = br + find(&bytes[br..end], b"[").unwrap();
    let b = a + find(&bytes[a..end], b"]").unwrap() + 1;
    let range = format!("[0 {left} {right} {}]", bytes.len() - right);
    ensure!(range.len() <= b - a, "PDF 签名 ByteRange 空间不足");
    bytes[a..b].fill(b' ');
    bytes[a..a + range.len()].copy_from_slice(range.as_bytes());
    let data = [&bytes[..left], &bytes[right..]].concat();
    let signature = identity.detached(&data)?;
    ensure!(signature.len() <= reserved, "证书链过大，无法写入 PDF");
    let hex = hex::encode_upper(signature);
    bytes[left + 1..left + 1 + hex.len()].copy_from_slice(hex.as_bytes());
    let result = verify_pdf(&bytes)?;
    ensure!(
        result.len() == 1 && result[0].file_fully_covered,
        "生成签名自检失败"
    );
    Ok(bytes)
}
fn pdf_string(s: &str) -> lopdf::Object {
    let mut bytes = vec![254, 255];
    for c in s.encode_utf16() {
        bytes.extend_from_slice(&c.to_be_bytes());
    }
    lopdf::Object::string_literal(bytes)
}
fn find(bytes: &[u8], pattern: &[u8]) -> Option<usize> {
    bytes.windows(pattern.len()).position(|w| w == pattern)
}
pub fn verify_pdf(bytes: &[u8]) -> Result<Vec<Verification>> {
    use lopdf::Object;
    let pdf = lopdf::Document::load_mem(bytes)?;
    let mut result = vec![];
    for obj in pdf.objects.values() {
        let Ok(dict) = obj.as_dict() else { continue };
        if dict.get(b"Type").and_then(Object::as_name).ok() != Some(b"Sig") {
            continue;
        }
        ensure!(
            dict.get(b"SubFilter").and_then(Object::as_name).ok() == Some(b"adbe.pkcs7.detached"),
            "此 PDF 签名类型暂不支持验签"
        );
        let nums = dict
            .get(b"ByteRange")?
            .as_array()?
            .iter()
            .map(|o| o.as_i64())
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ensure!(
            nums.len() == 4 && nums.iter().all(|n| *n >= 0),
            "ByteRange 无效"
        );
        let (start, len, gap_end, last) = (
            nums[0] as usize,
            nums[1] as usize,
            nums[2] as usize,
            nums[3] as usize,
        );
        ensure!(
            start == 0
                && len < gap_end
                && gap_end
                    .checked_add(last)
                    .is_some_and(|end| end <= bytes.len()),
            "签名范围越界"
        );
        ensure!(
            bytes.get(len) == Some(&b'<') && bytes.get(gap_end - 1) == Some(&b'>'),
            "签名排除区不是 Contents"
        );
        let gap = &bytes[len + 1..gap_end - 1];
        let hex: Vec<_> = gap
            .iter()
            .copied()
            .filter(|b| !b.is_ascii_whitespace())
            .collect();
        let raw = hex::decode(hex)?;
        let content = dict.get(b"Contents")?.as_str()?;
        ensure!(
            raw == content,
            "排除区不对应签名 Contents，可能存在覆盖范围伪造"
        );
        let size = der_length(content)?;
        let data = [&bytes[..len], &bytes[gap_end..gap_end + last]].concat();
        let mut verified = verify_detached(&data, &content[..size])?;
        verified.file_fully_covered = gap_end + last == bytes.len();
        result.push(verified);
    }
    ensure!(!result.is_empty(), "PDF 没有可验证的数字签名");
    Ok(result)
}
fn der_length(bytes: &[u8]) -> Result<usize> {
    ensure!(
        bytes.len() >= 2 && bytes[0] == 0x30,
        "签名不是 DER Sequence"
    );
    let count = if bytes[1] < 128 {
        0
    } else {
        (bytes[1] & 127) as usize
    };
    ensure!(count <= 4 && bytes.len() >= count + 2, "DER 长度无效");
    let len = if count == 0 {
        bytes[1] as usize
    } else {
        bytes[2..2 + count]
            .iter()
            .fold(0usize, |v, b| v * 256 + *b as usize)
    };
    let total = 2 + count + len;
    ensure!(total <= bytes.len(), "DER 签名不完整");
    Ok(total)
}
