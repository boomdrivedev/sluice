//! A key-service signer's records (78, 141; record 0119; design D7 of the
//! change `a-codebuild-fleet-runs-a-queues-checks`), as timber's
//! key-service signer signs its claims (timber C.7, timber record 0017):
//! an `ECC_NIST_P256` key in AWS KMS signs, by `ECDSA_SHA_256`, the
//! SHA-256 of a DSSE envelope's pre-authentication encoding, sent as a
//! digest; the envelope carries KMS's DER signature as answered, under the
//! key's ARN as its `keyid`. Every host verifies one with the public half
//! the policy lists, its S made low first, since KMS does not promise a low
//! S and timber reads it so, calling no service.

use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use p256::ecdsa::signature::Verifier as _;
use p256::ecdsa::{Signature, VerifyingKey};
use p256::pkcs8::DecodePublicKey as _;
use sha2::{Digest as _, Sha256};

use super::dsse::Envelope;
use super::statement::Statement;

/// The signing algorithm a key-service signer's records are made by.
pub const ALGORITHM: &str = "ECDSA_SHA_256";

/// What a key in a key service signs for `envelope`: the SHA-256 of its
/// pre-authentication encoding, and nothing of its statement.
pub fn digest(envelope: &Envelope) -> Result<[u8; 32]> {
    Ok(Sha256::digest(envelope.signed_bytes()?).into())
}

/// `statement`'s envelope signed by `key`, a key's ARN: `sign` is given the
/// digest of its pre-authentication encoding and answers the key's DER
/// signature, which the envelope carries as answered under the ARN.
pub fn envelope_of(
    statement: &Statement,
    key: &str,
    sign: impl FnOnce(&[u8; 32]) -> Result<Vec<u8>>,
) -> Result<Envelope> {
    let mut envelope = statement.envelope()?;
    let der = sign(&digest(&envelope)?)?;
    Signature::from_der(&der).map_err(|_| {
        anyhow!("the key {key} answered a signature that is no DER-encoded ECDSA signature")
    })?;
    envelope.sign_with(key, &der);
    Ok(envelope)
}

/// timber record 0017 (design D7): `statement`'s envelope signed by `key`,
/// a key in KMS by its ARN, through `aws`: one `Sign` of `ECDSA_SHA_256`
/// over the 32 bytes of the encoding's SHA-256, sent as a digest, so
/// nothing of the statement leaves the host.
pub fn sign(
    aws: &dyn crate::profiles::codebuild::aws::Aws,
    key: &str,
    statement: &Statement,
) -> Result<Envelope> {
    envelope_of(statement, key, |digest| aws.sign(key, digest))
}

/// A key-service signer's public half as the policy lists it: the base64
/// of its DER SubjectPublicKeyInfo, a P-256 key.
pub fn public_half(listed: &str) -> Result<VerifyingKey> {
    let der = STANDARD
        .decode(listed.trim())
        .context("the public half is not base64")?;
    VerifyingKey::from_public_key_der(&der)
        .map_err(|e| anyhow!("the public half is no P-256 public key: {e}"))
}

/// 78, 79: whether `envelope` is signed by `key`, a key's ARN, whose public
/// half is `public`: one of its signatures names the ARN as its `keyid` and
/// verifies, its S made low, over exactly its payload and payload type.
/// Otherwise why it counts for nothing.
pub fn verify(envelope: &Envelope, key: &str, public: &VerifyingKey) -> Result<()> {
    let signed = envelope.signed_bytes()?;
    let under: Vec<_> = envelope
        .signatures
        .iter()
        .filter(|s| s.keyid == key)
        .collect();
    if under.is_empty() {
        let named: Vec<&str> = envelope
            .signatures
            .iter()
            .map(|s| match s.keyid.as_str() {
                "" => "no key",
                k => k,
            })
            .collect();
        bail!(
            "its signatures name {}, not the key {key}",
            match named.is_empty() {
                true => "nothing".to_string(),
                false => named.join(", "),
            }
        );
    }
    for s in under {
        let Ok(bytes) = s.bytes() else { continue };
        let Ok(sig) = Signature::from_der(&bytes) else {
            continue;
        };
        let sig = sig.normalize_s().unwrap_or(sig);
        if public.verify(&signed, &sig).is_ok() {
            return Ok(());
        }
    }
    bail!("its signature under the key {key} does not verify by the public half the policy lists")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::signing::statement::tests::run;
    use p256::ecdsa::signature::hazmat::PrehashSigner as _;
    use p256::ecdsa::SigningKey;

    pub const KEY: &str =
        "arn:aws:kms:us-east-1:111122223333:key/1234abcd-12ab-34cd-56ef-1234567890ab";

    /// What KMS does with a digest: an ECDSA signature over it, DER.
    fn kms_signs(key: &SigningKey, digest: &[u8; 32]) -> Vec<u8> {
        let sig: Signature = key.sign_prehash(digest).unwrap();
        sig.to_der().as_bytes().to_vec()
    }

    /// `key`'s public half as KMS's `GetPublicKey` answers it and the
    /// policy lists it: the base64 of its DER SubjectPublicKeyInfo.
    pub fn listed(key: &SigningKey) -> String {
        STANDARD.encode(spki(key))
    }

    /// `key`'s DER SubjectPublicKeyInfo: `id-ecPublicKey` on `prime256v1`
    /// and its point, uncompressed.
    pub fn spki(key: &SigningKey) -> Vec<u8> {
        const PREFIX: &[u8] = &[
            0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06,
            0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
        ];
        let point = key.verifying_key().to_encoded_point(false);
        [PREFIX, point.as_bytes()].concat()
    }

    /// Record 0119 (design D7): a `kms` signer's envelope holds the run's
    /// statement, its `keyid` the key's ARN and its `sig` the DER bytes the
    /// key answered over the encoding's digest, and verifies by the listed
    /// half alone; another statement under the same signature does not.
    #[test]
    fn a_kms_envelope_holds_the_runs_statement() {
        let key = SigningKey::random(&mut rand_core::OsRng);
        let statement = Statement::of(run());
        let mut asked = None;
        let envelope = envelope_of(&statement, KEY, |digest| {
            asked = Some(*digest);
            Ok(kms_signs(&key, digest))
        })
        .unwrap();
        assert_eq!(asked.unwrap(), digest(&envelope).unwrap());
        assert_eq!(envelope.signatures.len(), 1);
        assert_eq!(envelope.signatures[0].keyid, KEY);
        let der = envelope.signatures[0].bytes().unwrap();
        Signature::from_der(&der).unwrap();
        assert_eq!(Statement::from_envelope(&envelope).unwrap(), statement);
        let half = public_half(&listed(&key)).unwrap();
        verify(&envelope, KEY, &half).unwrap();

        let mut forged = Statement::of(run()).envelope().unwrap();
        forged.payload = STANDARD.encode(b"{\"outcome\":\"pass\"}");
        forged.signatures = envelope.signatures.clone();
        let e = verify(&forged, KEY, &half).unwrap_err().to_string();
        assert!(e.contains("does not verify"), "{e}");
        let other = "arn:aws:kms:us-east-1:111122223333:key/other";
        let e = verify(&envelope, other, &half).unwrap_err().to_string();
        assert!(e.contains(KEY) && e.contains(other), "{e}");
        let stranger = SigningKey::random(&mut rand_core::OsRng);
        let e = verify(&envelope, KEY, stranger.verifying_key())
            .unwrap_err()
            .to_string();
        assert!(e.contains("does not verify"), "{e}");
        assert!(envelope_of(&statement, KEY, |_| Ok(b"not der".to_vec())).is_err());
    }

    /// timber record 0017 (design D7): the sign asks KMS for one signature
    /// of the key, by its ARN, over the 32 bytes of the encoding's SHA-256,
    /// and the envelope carries the answer under that ARN, verifying by the
    /// key's public half.
    #[test]
    fn a_kms_sign_sends_the_encodings_digest() {
        use crate::profiles::codebuild::aws::Aws as _;
        use crate::profiles::codebuild::fake::Fake;
        let aws = Fake::new().with_key("alias/sluice-sign");
        let arn = aws.get_public_key("alias/sluice-sign").unwrap().arn;
        let statement = Statement::of(run());
        let envelope = sign(&aws, &arn, &statement).unwrap();
        let calls = aws.calls();
        let signs: Vec<&String> = calls.iter().filter(|c| c.starts_with("Sign ")).collect();
        let sent = crate::profiles::remote_refs::sha256_hex(&digest(&envelope).unwrap());
        assert_eq!(signs, [&format!("Sign {arn} {sent}")]);
        assert_eq!(envelope.signatures.len(), 1);
        assert_eq!(envelope.signatures[0].keyid, arn);
        assert_eq!(Statement::from_envelope(&envelope).unwrap(), statement);
        let der = aws.get_public_key(&arn).unwrap().der;
        let half = public_half(&STANDARD.encode(der)).unwrap();
        verify(&envelope, &arn, &half).unwrap();
        // A key KMS does not hold signs nothing.
        assert!(sign(
            &aws,
            "arn:aws:kms:us-east-1:111122223333:key/other",
            &statement
        )
        .is_err());
    }

    /// timber record 0017: KMS does not promise a low S; a signature with a
    /// high S verifies once it is normalized, as timber reads it.
    #[test]
    fn a_high_s_signature_verifies_once_normalized() {
        let key = SigningKey::random(&mut rand_core::OsRng);
        let statement = Statement::of(run());
        let envelope = envelope_of(&statement, KEY, |digest| {
            let sig: Signature = key.sign_prehash(digest).unwrap();
            let low = sig.normalize_s().unwrap_or(sig);
            let (r, s) = low.split_scalars();
            let high = Signature::from_scalars(r, -s).unwrap();
            assert!(high.normalize_s().is_some(), "the S made is high");
            Ok(high.to_der().as_bytes().to_vec())
        })
        .unwrap();
        let half = public_half(&listed(&key)).unwrap();
        let der = envelope.signatures[0].bytes().unwrap();
        assert!(Signature::from_der(&der).unwrap().normalize_s().is_some());
        verify(&envelope, KEY, &half).unwrap();
    }
}
