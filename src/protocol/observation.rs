//! NautilusTrader-scoped observations, provenance, omissions, and retention metadata.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    canonical,
    capability::ObservationCapability,
    identity::ObservationId,
    live::LiveObservation,
    value::{ContentDigest, FieldPath, InstrumentId, PositionId, TimestampNs},
    version::ProtocolVersion,
};

/// Reports a structural or digest error in an observation.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ObservationError {
    /// The observation uses an unsupported protocol version.
    #[error("unsupported protocol version {version:?}")]
    UnsupportedVersion {
        /// The unsupported version.
        version: ProtocolVersion,
    },
    /// The observation expires before it is created.
    #[error("observation expiry precedes creation")]
    ExpiryBeforeCreation,
    /// The disclosed grant expires before the observation.
    #[error("capability grant expires before observation")]
    GrantExpiresBeforeObservation,
    /// A position summary is present without its observation capability.
    #[error("position summaries are present without a disclosed capability")]
    PositionCapabilityMissing,
    /// An instrument summary is present without its observation capability.
    #[error("instrument summaries are present without a disclosed capability")]
    InstrumentCapabilityMissing,
    /// A view refers to an instrument outside the disclosed scope.
    #[error("instrument {instrument_id} is outside the disclosed grant")]
    InstrumentOutsideGrant {
        /// The out-of-scope instrument.
        instrument_id: InstrumentId,
    },
    /// A position identity appears more than once.
    #[error("duplicate position {position_id}")]
    DuplicatePosition {
        /// The duplicate position.
        position_id: PositionId,
    },
    /// An instrument summary appears more than once.
    #[error("duplicate instrument {instrument_id}")]
    DuplicateInstrument {
        /// The duplicate instrument.
        instrument_id: InstrumentId,
    },
    /// A position update claims a time after observation creation.
    #[error("position {position_id} was updated after observation creation")]
    PositionUpdatedAfterCreation {
        /// The invalid position.
        position_id: PositionId,
    },
    /// The observation digest does not cover its current content.
    #[error("observation digest mismatch")]
    DigestMismatch {
        /// The digest calculated from current content.
        expected: ContentDigest,
        /// The digest supplied with the observation.
        actual: ContentDigest,
    },
    /// The observation has expired for the supplied evaluation time.
    #[error("observation expired at {expires_at}")]
    Expired {
        /// The observation expiry time.
        expires_at: TimestampNs,
    },
    /// Canonical observation serialization failed.
    #[error("canonical observation serialization failed: {message}")]
    Canonicalization {
        /// The serialization error.
        message: String,
    },
}

/// Contains one complete, versioned observation constructed by NautilusTrader.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    /// The external protocol version.
    pub version: ProtocolVersion,
    /// The observation identity.
    pub id: ObservationId,
    /// The observation construction time.
    pub created_at: TimestampNs,
    /// The last time the observation may be used.
    pub expires_at: TimestampNs,
    /// The effective scope disclosed for this observation.
    pub grant: super::capability::CapabilityGrant,
    /// The source and time for disclosed fields.
    pub provenance: Vec<SourceProvenance>,
    /// The fields NautilusTrader withheld or could not provide.
    pub omissions: Vec<FieldOmission>,
    /// The recording sensitivity assigned by NautilusTrader.
    pub retention: RetentionClass,
    /// The protocol payload.
    pub payload: ObservationPayload,
    /// The canonical SHA-256 digest over every other field.
    pub digest: ContentDigest,
}

impl Observation {
    /// Validates structural invariants and the content digest.
    pub fn validate(&self) -> Result<(), ObservationError> {
        if !self.version.is_supported() {
            return Err(ObservationError::UnsupportedVersion {
                version: self.version,
            });
        }

        if self.expires_at < self.created_at {
            return Err(ObservationError::ExpiryBeforeCreation);
        }

        if self.grant.expires_at < self.expires_at {
            return Err(ObservationError::GrantExpiresBeforeObservation);
        }
        self.validate_live_payload()?;
        let expected = self.computed_digest()?;
        if expected != self.digest {
            return Err(ObservationError::DigestMismatch {
                expected,
                actual: self.digest.clone(),
            });
        }
        Ok(())
    }

    /// Validates the observation and checks it against an evaluation time.
    pub fn validate_at(&self, now: TimestampNs) -> Result<(), ObservationError> {
        self.validate()?;

        if now > self.expires_at {
            return Err(ObservationError::Expired {
                expires_at: self.expires_at,
            });
        }
        Ok(())
    }

    /// Computes the canonical digest excluding the `digest` field itself.
    pub fn computed_digest(&self) -> Result<ContentDigest, ObservationError> {
        canonical::sha256(&ObservationDigest {
            version: self.version,
            id: &self.id,
            created_at: self.created_at,
            expires_at: self.expires_at,
            grant: &self.grant,
            provenance: &self.provenance,
            omissions: &self.omissions,
            retention: self.retention,
            payload: &self.payload,
        })
        .map_err(|e| ObservationError::Canonicalization {
            message: e.to_string(),
        })
    }

    /// Recomputes the content digest after local synthetic-data changes.
    pub fn refresh_digest(&mut self) -> Result<(), ObservationError> {
        self.digest = self.computed_digest()?;
        Ok(())
    }

    /// Returns the stable reference carried by traces and requests.
    #[must_use]
    pub fn reference(&self) -> ObservationRef {
        ObservationRef {
            id: self.id.clone(),
            digest: self.digest.clone(),
        }
    }

    fn validate_live_payload(&self) -> Result<(), ObservationError> {
        let ObservationPayload::Live(live) = &self.payload;
        if !live.positions.is_empty()
            && !self
                .grant
                .observations
                .contains(&ObservationCapability::PositionSummary)
        {
            return Err(ObservationError::PositionCapabilityMissing);
        }

        if !live.instruments.is_empty()
            && !self
                .grant
                .observations
                .contains(&ObservationCapability::InstrumentSummary)
        {
            return Err(ObservationError::InstrumentCapabilityMissing);
        }

        let mut position_ids = BTreeSet::new();
        for position in &live.positions {
            if !self.grant.instruments.contains(&position.instrument_id) {
                return Err(ObservationError::InstrumentOutsideGrant {
                    instrument_id: position.instrument_id.clone(),
                });
            }

            if !position_ids.insert(&position.position_id) {
                return Err(ObservationError::DuplicatePosition {
                    position_id: position.position_id.clone(),
                });
            }

            if position.updated_at > self.created_at {
                return Err(ObservationError::PositionUpdatedAfterCreation {
                    position_id: position.position_id.clone(),
                });
            }
        }

        let mut instrument_ids = BTreeSet::new();
        for instrument in &live.instruments {
            if !self.grant.instruments.contains(&instrument.instrument_id) {
                return Err(ObservationError::InstrumentOutsideGrant {
                    instrument_id: instrument.instrument_id.clone(),
                });
            }

            if !instrument_ids.insert(&instrument.instrument_id) {
                return Err(ObservationError::DuplicateInstrument {
                    instrument_id: instrument.instrument_id.clone(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct ObservationDigest<'a> {
    version: ProtocolVersion,
    id: &'a ObservationId,
    created_at: TimestampNs,
    expires_at: TimestampNs,
    grant: &'a super::capability::CapabilityGrant,
    provenance: &'a [SourceProvenance],
    omissions: &'a [FieldOmission],
    retention: RetentionClass,
    payload: &'a ObservationPayload,
}

/// Links a request or trace to exact observation content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObservationRef {
    /// The observation identity.
    pub id: ObservationId,
    /// The exact observation content digest.
    pub digest: ContentDigest,
}

/// Associates one field path with its source and observation time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceProvenance {
    /// The field or subtree supplied by the source.
    pub field: FieldPath,
    /// The source category.
    pub source: ProvenanceSource,
    /// The time the source value was observed.
    pub observed_at: TimestampNs,
    /// An optional source version or revision.
    pub version: Option<String>,
}

/// Names the authoritative source category for a public field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceSource {
    /// NautilusTrader's fresh position state.
    PositionState,
    /// NautilusTrader's instrument definition.
    InstrumentDefinition,
}

/// Records why NautilusTrader did not disclose a field or subtree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldOmission {
    /// The omitted field or subtree.
    pub field: FieldPath,
    /// The omission reason.
    pub reason: OmissionReason,
}

/// Explains why an observation field is absent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OmissionReason {
    /// The effective grant did not include the field.
    NotGranted,
    /// NautilusTrader could not obtain the field.
    Unavailable,
    /// NautilusTrader redacted the field.
    Redacted,
    /// The current protocol cannot represent the field.
    Unsupported,
}

/// Classifies observation data for caller-controlled recording.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RetentionClass {
    /// Callers should retain only an observation reference.
    ReferenceOnly,
    /// Callers may retain the derived public payload under their policy.
    Derived,
    /// Callers must not record the full observation through the SDK recorder.
    Restricted,
}

/// Contains the protocol-specific observation domain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ObservationPayload {
    /// Contains the live proposal-authoring view.
    Live(LiveObservation),
}
