use std::error::Error;
use std::fmt;

use ring::signature;
use sha2::{Digest, Sha256};

use crate::{
    ChromiumExtensionId, ChromiumManifestKeyDigest, MAX_CRX3_HEADER_BYTES,
    MAX_CRX3_PROOF_COMPONENT_BYTES, MAX_CRX3_SIGNATURE_PROOFS, MAX_EXTENSION_ARCHIVE_BYTES,
};

const CRX3_MAGIC: &[u8; 4] = b"Cr24";
const CRX3_VERSION: u32 = 3;
const CRX3_PREFIX_BYTES: usize = 12;
const SIGNATURE_CONTEXT: &[u8; 16] = b"CRX3 SignedData\0";
const SIGNED_HEADER_FIELD: u64 = 10_000;
const ZIP_LOCAL_FILE_MAGIC: &[u8; 4] = b"PK\x03\x04";

const RSA_ALGORITHM_IDENTIFIER: &[u8] = &[
    0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05, 0x00,
];
const P256_ALGORITHM_IDENTIFIER: &[u8] = &[
    0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d,
    0x03, 0x01, 0x07,
];

/// One CRX3 package whose signed header, archive, and developer identity agree.
///
/// Verification authenticates the immutable CRX bytes. It does not extract the
/// ZIP payload, admit its manifest, consult a release catalog, or grant product
/// authority.
#[derive(Clone, Debug)]
pub struct VerifiedCrx3Package<'a> {
    extension_id: ChromiumExtensionId,
    developer_key_sha256: ChromiumManifestKeyDigest,
    archive: &'a [u8],
    signature_proofs: usize,
}

impl<'a> VerifiedCrx3Package<'a> {
    /// Parses and verifies an exact CRX3 byte sequence.
    ///
    /// When supplied, `expected_id` must equal the identifier derived from the
    /// signed developer public key. Every recognized proof in the header must
    /// verify; a valid unrelated publisher proof cannot substitute for the
    /// developer proof named by `signed_header_data.crx_id`.
    pub fn parse_and_verify(
        bytes: &'a [u8],
        expected_id: Option<&ChromiumExtensionId>,
    ) -> Result<Self, Crx3PackageError> {
        let max_file_bytes = usize::try_from(MAX_EXTENSION_ARCHIVE_BYTES)
            .unwrap_or(usize::MAX)
            .saturating_add(MAX_CRX3_HEADER_BYTES)
            .saturating_add(CRX3_PREFIX_BYTES);
        if bytes.len() > max_file_bytes {
            return Err(Crx3PackageError::PackageTooLarge);
        }
        let prefix = bytes
            .get(..CRX3_PREFIX_BYTES)
            .ok_or(Crx3PackageError::Truncated)?;
        if &prefix[..4] != CRX3_MAGIC {
            return Err(Crx3PackageError::Magic);
        }
        if read_u32(&prefix[4..8]) != CRX3_VERSION {
            return Err(Crx3PackageError::Version);
        }
        let header_length =
            usize::try_from(read_u32(&prefix[8..12])).map_err(|_| Crx3PackageError::HeaderSize)?;
        if header_length == 0 || header_length > MAX_CRX3_HEADER_BYTES {
            return Err(Crx3PackageError::HeaderSize);
        }
        let archive_offset = CRX3_PREFIX_BYTES
            .checked_add(header_length)
            .ok_or(Crx3PackageError::HeaderSize)?;
        let header = bytes
            .get(CRX3_PREFIX_BYTES..archive_offset)
            .ok_or(Crx3PackageError::Truncated)?;
        let archive = bytes
            .get(archive_offset..)
            .ok_or(Crx3PackageError::Truncated)?;
        if archive.is_empty()
            || archive.len() as u64 > MAX_EXTENSION_ARCHIVE_BYTES
            || !archive.starts_with(ZIP_LOCAL_FILE_MAGIC)
        {
            return Err(Crx3PackageError::Archive);
        }

        let parsed = parse_header(header)?;
        let signed_header = parsed
            .signed_header
            .ok_or(Crx3PackageError::SignedHeaderMissing)?;
        let declared_id = parse_signed_data(signed_header)?;
        let message = signed_message(signed_header, archive)?;

        let mut developer = None;
        for proof in &parsed.proofs {
            let public_key = proof.verification_key()?;
            let verifier =
                signature::UnparsedPublicKey::new(proof.algorithm.verifier(), public_key);
            verifier
                .verify(&message, proof.signature)
                .map_err(|_| Crx3PackageError::Signature)?;

            let digest: [u8; 32] = Sha256::digest(proof.public_key).into();
            if digest[..16] == declared_id {
                if developer.is_some() {
                    return Err(Crx3PackageError::DuplicateDeveloperProof);
                }
                developer = Some((
                    digest,
                    ChromiumManifestKeyDigest::from_bytes(digest).derived_extension_id(),
                ));
            }
        }

        let (developer_key_sha256, extension_id) =
            developer.ok_or(Crx3PackageError::DeveloperProofMissing)?;
        if expected_id.is_some_and(|expected| *expected != extension_id) {
            return Err(Crx3PackageError::ExpectedIdMismatch);
        }

        Ok(Self {
            extension_id,
            developer_key_sha256: ChromiumManifestKeyDigest::from_bytes(developer_key_sha256),
            archive,
            signature_proofs: parsed.proofs.len(),
        })
    }

    /// Returns the identifier derived from the signed developer key.
    pub const fn extension_id(&self) -> &ChromiumExtensionId {
        &self.extension_id
    }

    /// Returns SHA-256 of the signed developer public-key bytes.
    pub const fn developer_key_sha256(&self) -> ChromiumManifestKeyDigest {
        self.developer_key_sha256
    }

    /// Borrows the authenticated ZIP payload without extracting it.
    pub const fn archive_bytes(&self) -> &'a [u8] {
        self.archive
    }

    /// Returns the number of cryptographic proofs verified in the CRX3 header.
    pub const fn signature_proof_count(&self) -> usize {
        self.signature_proofs
    }
}

#[derive(Clone, Copy, Debug)]
enum ProofAlgorithm {
    Rsa,
    EcdsaP256,
}

impl ProofAlgorithm {
    fn verifier(self) -> &'static dyn signature::VerificationAlgorithm {
        match self {
            Self::Rsa => &signature::RSA_PKCS1_2048_8192_SHA256,
            Self::EcdsaP256 => &signature::ECDSA_P256_SHA256_ASN1,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Proof<'a> {
    algorithm: ProofAlgorithm,
    public_key: &'a [u8],
    signature: &'a [u8],
}

impl Proof<'_> {
    fn verification_key(&self) -> Result<&[u8], Crx3PackageError> {
        let algorithm_identifier = match self.algorithm {
            ProofAlgorithm::Rsa => RSA_ALGORITHM_IDENTIFIER,
            ProofAlgorithm::EcdsaP256 => P256_ALGORITHM_IDENTIFIER,
        };
        let key = parse_subject_public_key(self.public_key, algorithm_identifier)?;
        if matches!(self.algorithm, ProofAlgorithm::EcdsaP256)
            && (key.len() != 65 || key.first() != Some(&0x04))
        {
            return Err(Crx3PackageError::PublicKey);
        }
        Ok(key)
    }
}

struct ParsedHeader<'a> {
    proofs: Vec<Proof<'a>>,
    signed_header: Option<&'a [u8]>,
}

fn parse_header(bytes: &[u8]) -> Result<ParsedHeader<'_>, Crx3PackageError> {
    let mut cursor = ProtoCursor::new(bytes);
    let mut proofs = Vec::new();
    let mut signed_header = None;
    while let Some(field) = cursor.next()? {
        match (field.number, field.value) {
            (2, ProtoValue::Bytes(value)) => {
                push_proof(&mut proofs, parse_proof(value, ProofAlgorithm::Rsa)?)?
            }
            (3, ProtoValue::Bytes(value)) => {
                push_proof(&mut proofs, parse_proof(value, ProofAlgorithm::EcdsaP256)?)?
            }
            (SIGNED_HEADER_FIELD, ProtoValue::Bytes(_)) if signed_header.is_some() => {
                return Err(Crx3PackageError::DuplicateSignedHeader);
            }
            (SIGNED_HEADER_FIELD, ProtoValue::Bytes(value)) => signed_header = Some(value),
            _ => {}
        }
    }
    if proofs.is_empty() {
        return Err(Crx3PackageError::ProofMissing);
    }
    Ok(ParsedHeader {
        proofs,
        signed_header,
    })
}

fn push_proof<'a>(proofs: &mut Vec<Proof<'a>>, proof: Proof<'a>) -> Result<(), Crx3PackageError> {
    if proofs.len() >= MAX_CRX3_SIGNATURE_PROOFS {
        return Err(Crx3PackageError::ProofCount);
    }
    proofs.push(proof);
    Ok(())
}

fn parse_proof(bytes: &[u8], algorithm: ProofAlgorithm) -> Result<Proof<'_>, Crx3PackageError> {
    let mut cursor = ProtoCursor::new(bytes);
    let mut public_key = None;
    let mut signature = None;
    while let Some(field) = cursor.next()? {
        match (field.number, field.value) {
            (1, ProtoValue::Bytes(_)) if public_key.is_some() => {
                return Err(Crx3PackageError::Proof);
            }
            (1, ProtoValue::Bytes(value)) => public_key = Some(value),
            (2, ProtoValue::Bytes(_)) if signature.is_some() => {
                return Err(Crx3PackageError::Proof);
            }
            (2, ProtoValue::Bytes(value)) => signature = Some(value),
            _ => {}
        }
    }
    let public_key = public_key.ok_or(Crx3PackageError::Proof)?;
    let signature = signature.ok_or(Crx3PackageError::Proof)?;
    if public_key.is_empty()
        || signature.is_empty()
        || public_key.len() > MAX_CRX3_PROOF_COMPONENT_BYTES
        || signature.len() > MAX_CRX3_PROOF_COMPONENT_BYTES
    {
        return Err(Crx3PackageError::Proof);
    }
    Ok(Proof {
        algorithm,
        public_key,
        signature,
    })
}

fn parse_signed_data(bytes: &[u8]) -> Result<[u8; 16], Crx3PackageError> {
    let mut cursor = ProtoCursor::new(bytes);
    let mut crx_id = None;
    while let Some(field) = cursor.next()? {
        if let (1, ProtoValue::Bytes(value)) = (field.number, field.value) {
            if value.len() != 16 || crx_id.is_some() {
                return Err(Crx3PackageError::SignedHeader);
            }
            let mut id = [0_u8; 16];
            id.copy_from_slice(value);
            crx_id = Some(id);
        }
    }
    crx_id.ok_or(Crx3PackageError::SignedHeader)
}

fn signed_message(signed_header: &[u8], archive: &[u8]) -> Result<Vec<u8>, Crx3PackageError> {
    let signed_header_length =
        u32::try_from(signed_header.len()).map_err(|_| Crx3PackageError::SignedHeader)?;
    let length = SIGNATURE_CONTEXT
        .len()
        .checked_add(4)
        .and_then(|value| value.checked_add(signed_header.len()))
        .and_then(|value| value.checked_add(archive.len()))
        .ok_or(Crx3PackageError::PackageTooLarge)?;
    let mut message = Vec::with_capacity(length);
    message.extend_from_slice(SIGNATURE_CONTEXT);
    message.extend_from_slice(&signed_header_length.to_le_bytes());
    message.extend_from_slice(signed_header);
    message.extend_from_slice(archive);
    Ok(message)
}

fn parse_subject_public_key<'a>(
    bytes: &'a [u8],
    expected_algorithm: &[u8],
) -> Result<&'a [u8], Crx3PackageError> {
    let mut outer = DerCursor::new(bytes);
    let sequence = outer.read(0x30)?;
    if !outer.is_empty() {
        return Err(Crx3PackageError::PublicKey);
    }
    let mut sequence = DerCursor::new(sequence);
    let algorithm = sequence.read(0x30)?;
    if algorithm != expected_algorithm {
        return Err(Crx3PackageError::PublicKey);
    }
    let bit_string = sequence.read(0x03)?;
    if !sequence.is_empty() || bit_string.first() != Some(&0) {
        return Err(Crx3PackageError::PublicKey);
    }
    bit_string.get(1..).ok_or(Crx3PackageError::PublicKey)
}

struct DerCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> DerCursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn read(&mut self, expected_tag: u8) -> Result<&'a [u8], Crx3PackageError> {
        if self.bytes.get(self.offset).copied() != Some(expected_tag) {
            return Err(Crx3PackageError::PublicKey);
        }
        self.offset += 1;
        let first = *self
            .bytes
            .get(self.offset)
            .ok_or(Crx3PackageError::PublicKey)?;
        self.offset += 1;
        let length = if first & 0x80 == 0 {
            usize::from(first)
        } else {
            let octets = usize::from(first & 0x7f);
            if octets == 0 || octets > 4 {
                return Err(Crx3PackageError::PublicKey);
            }
            let encoded = self
                .bytes
                .get(self.offset..self.offset + octets)
                .ok_or(Crx3PackageError::PublicKey)?;
            if encoded.first() == Some(&0) {
                return Err(Crx3PackageError::PublicKey);
            }
            self.offset += octets;
            let mut value = 0_usize;
            for byte in encoded {
                value = value
                    .checked_mul(256)
                    .and_then(|value| value.checked_add(usize::from(*byte)))
                    .ok_or(Crx3PackageError::PublicKey)?;
            }
            if value < 128 {
                return Err(Crx3PackageError::PublicKey);
            }
            value
        };
        let end = self
            .offset
            .checked_add(length)
            .ok_or(Crx3PackageError::PublicKey)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(Crx3PackageError::PublicKey)?;
        self.offset = end;
        Ok(value)
    }
}

struct ProtoField<'a> {
    number: u64,
    value: ProtoValue<'a>,
}

enum ProtoValue<'a> {
    Varint,
    Bytes(&'a [u8]),
    Fixed,
}

struct ProtoCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> ProtoCursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn next(&mut self) -> Result<Option<ProtoField<'a>>, Crx3PackageError> {
        if self.offset == self.bytes.len() {
            return Ok(None);
        }
        let key = self.varint()?;
        let number = key >> 3;
        if number == 0 {
            return Err(Crx3PackageError::Protobuf);
        }
        let value = match key & 7 {
            0 => {
                self.varint()?;
                ProtoValue::Varint
            }
            1 => {
                self.advance(8)?;
                ProtoValue::Fixed
            }
            2 => {
                let encoded_length = self.varint()?;
                let length =
                    usize::try_from(encoded_length).map_err(|_| Crx3PackageError::Protobuf)?;
                let end = self
                    .offset
                    .checked_add(length)
                    .ok_or(Crx3PackageError::Protobuf)?;
                let bytes = self
                    .bytes
                    .get(self.offset..end)
                    .ok_or(Crx3PackageError::Protobuf)?;
                self.offset = end;
                ProtoValue::Bytes(bytes)
            }
            5 => {
                self.advance(4)?;
                ProtoValue::Fixed
            }
            _ => return Err(Crx3PackageError::Protobuf),
        };
        Ok(Some(ProtoField { number, value }))
    }

    fn varint(&mut self) -> Result<u64, Crx3PackageError> {
        let mut value = 0_u64;
        for shift in (0..70).step_by(7) {
            let byte = *self
                .bytes
                .get(self.offset)
                .ok_or(Crx3PackageError::Protobuf)?;
            self.offset += 1;
            if shift == 63 && byte > 1 {
                return Err(Crx3PackageError::Protobuf);
            }
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(Crx3PackageError::Protobuf)
    }

    fn advance(&mut self, bytes: usize) -> Result<(), Crx3PackageError> {
        self.offset = self
            .offset
            .checked_add(bytes)
            .filter(|offset| *offset <= self.bytes.len())
            .ok_or(Crx3PackageError::Protobuf)?;
        Ok(())
    }
}

fn read_u32(bytes: &[u8]) -> u32 {
    let mut value = [0_u8; 4];
    value.copy_from_slice(bytes);
    u32::from_le_bytes(value)
}

/// Stable CRX3 authentication rejection reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Crx3PackageError {
    /// Exact CRX bytes exceed the package ceiling.
    PackageTooLarge,
    /// The CRX prefix, header, or archive is truncated.
    Truncated,
    /// The CRX magic is not `Cr24`.
    Magic,
    /// The package is not CRX3.
    Version,
    /// The encoded header length is zero, excessive, or overflows.
    HeaderSize,
    /// The payload is not a bounded non-empty ZIP archive.
    Archive,
    /// The header protocol buffer is malformed or uses unsupported wire groups.
    Protobuf,
    /// The header contains no recognized signature proof.
    ProofMissing,
    /// The header contains too many signature proofs.
    ProofCount,
    /// A signature proof is incomplete, duplicated, empty, or excessive.
    Proof,
    /// The CRX signed-header field is absent.
    SignedHeaderMissing,
    /// The CRX signed-header field occurs more than once.
    DuplicateSignedHeader,
    /// Signed header data is malformed or lacks one exact 16-byte CRX id.
    SignedHeader,
    /// A proof public key is not an exact supported DER SPKI value.
    PublicKey,
    /// A recognized CRX proof does not verify over the exact signed bytes.
    Signature,
    /// No verified proof's public key derives the declared CRX id.
    DeveloperProofMissing,
    /// More than one verified proof derives the declared CRX id.
    DuplicateDeveloperProof,
    /// The signed developer id differs from the caller's expected id.
    ExpectedIdMismatch,
}

impl fmt::Display for Crx3PackageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::PackageTooLarge => "CRX3 package exceeds the hard byte ceiling",
            Self::Truncated => "CRX3 package is truncated",
            Self::Magic => "CRX3 package magic is invalid",
            Self::Version => "CRX package version is unsupported",
            Self::HeaderSize => "CRX3 header size is invalid",
            Self::Archive => "CRX3 archive payload is invalid",
            Self::Protobuf => "CRX3 header protocol buffer is malformed",
            Self::ProofMissing => "CRX3 header has no recognized signature proof",
            Self::ProofCount => "CRX3 header has too many signature proofs",
            Self::Proof => "CRX3 signature proof is malformed",
            Self::SignedHeaderMissing => "CRX3 signed header is absent",
            Self::DuplicateSignedHeader => "CRX3 signed header is duplicated",
            Self::SignedHeader => "CRX3 signed header is malformed",
            Self::PublicKey => "CRX3 proof public key is malformed or unsupported",
            Self::Signature => "CRX3 signature verification failed",
            Self::DeveloperProofMissing => "CRX3 developer proof is absent",
            Self::DuplicateDeveloperProof => "CRX3 developer proof is ambiguous",
            Self::ExpectedIdMismatch => "CRX3 developer key does not derive the expected id",
        })
    }
}

impl Error for Crx3PackageError {}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use ring::rand::SystemRandom;
    use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_ASN1_SIGNING};

    fn push_varint(bytes: &mut Vec<u8>, mut value: u64) {
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            bytes.push(byte);
            if value == 0 {
                return;
            }
        }
    }

    fn push_bytes_field(bytes: &mut Vec<u8>, number: u64, value: &[u8]) {
        push_varint(bytes, (number << 3) | 2);
        push_varint(bytes, value.len() as u64);
        bytes.extend_from_slice(value);
    }

    fn p256_spki(point: &[u8]) -> Vec<u8> {
        assert_eq!(point.len(), 65);
        let mut spki = vec![0x30, 0x59, 0x30, 0x13];
        spki.extend_from_slice(P256_ALGORITHM_IDENTIFIER);
        spki.extend_from_slice(&[0x03, 0x42, 0x00]);
        spki.extend_from_slice(point);
        spki
    }

    fn signed_fixture() -> (Vec<u8>, ChromiumExtensionId) {
        let random = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &random).unwrap();
        let pair =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &random)
                .unwrap();
        let public_key = p256_spki(pair.public_key().as_ref());
        let digest: [u8; 32] = Sha256::digest(&public_key).into();
        let extension_id = ChromiumManifestKeyDigest::from_bytes(digest).derived_extension_id();

        let mut signed_header = Vec::new();
        push_bytes_field(&mut signed_header, 1, &digest[..16]);
        let archive = b"PK\x03\x04zephium-crx3-fixture";
        let message = signed_message(&signed_header, archive).unwrap();
        let signature = pair.sign(&random, &message).unwrap();

        let mut proof = Vec::new();
        push_bytes_field(&mut proof, 1, &public_key);
        push_bytes_field(&mut proof, 2, signature.as_ref());
        let mut header = Vec::new();
        push_bytes_field(&mut header, 3, &proof);
        push_bytes_field(&mut header, SIGNED_HEADER_FIELD, &signed_header);

        let mut crx = Vec::new();
        crx.extend_from_slice(CRX3_MAGIC);
        crx.extend_from_slice(&CRX3_VERSION.to_le_bytes());
        crx.extend_from_slice(&(header.len() as u32).to_le_bytes());
        crx.extend_from_slice(&header);
        crx.extend_from_slice(archive);
        (crx, extension_id)
    }

    #[test]
    fn verifies_exact_signed_archive_and_expected_identity() {
        let (bytes, expected) = signed_fixture();
        let package = VerifiedCrx3Package::parse_and_verify(&bytes, Some(&expected)).unwrap();
        assert_eq!(package.extension_id(), &expected);
        assert_eq!(package.signature_proof_count(), 1);
        assert!(package.archive_bytes().starts_with(ZIP_LOCAL_FILE_MAGIC));
    }

    #[test]
    fn rejects_archive_or_signature_mutation() {
        let (mut bytes, _) = signed_fixture();
        *bytes.last_mut().unwrap() ^= 1;
        assert_eq!(
            VerifiedCrx3Package::parse_and_verify(&bytes, None).unwrap_err(),
            Crx3PackageError::Signature
        );
    }

    #[test]
    fn rejects_wrong_expected_identity() {
        let (bytes, _) = signed_fixture();
        let wrong = ChromiumExtensionId::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        assert_eq!(
            VerifiedCrx3Package::parse_and_verify(&bytes, Some(&wrong)).unwrap_err(),
            Crx3PackageError::ExpectedIdMismatch
        );
    }

    #[test]
    fn rejects_oversized_and_malformed_headers_before_crypto() {
        let mut bytes = Vec::from(&b"Cr24\x03\0\0\0\0\0\0\0PK\x03\x04"[..]);
        assert_eq!(
            VerifiedCrx3Package::parse_and_verify(&bytes, None).unwrap_err(),
            Crx3PackageError::HeaderSize
        );
        bytes[8..12].copy_from_slice(&((MAX_CRX3_HEADER_BYTES as u32) + 1).to_le_bytes());
        assert_eq!(
            VerifiedCrx3Package::parse_and_verify(&bytes, None).unwrap_err(),
            Crx3PackageError::HeaderSize
        );
    }

    proptest! {
        #[test]
        fn arbitrary_bounded_input_is_total(bytes in prop::collection::vec(any::<u8>(), 0..=4096)) {
            let _ = VerifiedCrx3Package::parse_and_verify(&bytes, None);
        }
    }
}
