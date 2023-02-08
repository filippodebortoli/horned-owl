use crate::model::ForIRI;
use oxiri::{IriParseError, IriRef};
use std::{
    borrow::Borrow,
    fmt::{Debug, Display},
    hash::Hash,
    ops::Deref,
};

pub trait ResourceIdentifiable: ForIRI + Deref<Target = str> {}

#[derive(Clone, Debug)]
pub enum IRIMaybe<T: ForIRI> {
    Unchecked(T),
    Validated(IriRef<T>),
}

impl<T: Default + ForIRI> Default for IRIMaybe<T> {
    fn default() -> Self {
        Self::Unchecked(T::default())
    }
}

impl<T: ForIRI> IRIMaybe<T> {
    pub fn new_unchecked(t: T) -> Self {
        Self::Unchecked(t)
    }
}

impl<T: Deref<Target = str> + ForIRI> IRIMaybe<T> {
    pub fn new_validated(t: T) -> Result<Self, IriParseError> {
        Ok(Self::Validated(IriRef::parse(t)?))
    }

    pub fn get_inner(&self) -> &str {
        match self {
            IRIMaybe::Unchecked(t) => t,
            IRIMaybe::Validated(iri) => iri.as_str(),
        }
    }

    pub fn into_inner(self) -> T {
        match self {
            IRIMaybe::Unchecked(t) => t,
            IRIMaybe::Validated(t) => t.into_inner(),
        }
    }

    pub fn to_validated(self) -> Result<Self, IriParseError> {
        if let IRIMaybe::Unchecked(t) = self {
            Self::new_validated(t)
        } else {
            Ok(self)
        }
    }

    pub fn get_validated(&self) -> Option<&IriRef<T>> {
        match &self {
            IRIMaybe::Unchecked(_) => None,
            IRIMaybe::Validated(iri) => Some(iri),
        }
    }

    pub fn is_validated(&self) -> bool {
        self.get_validated().is_some()
    }
}

impl<T: Deref<Target = str> + ForIRI> AsRef<str> for IRIMaybe<T> {
    fn as_ref(&self) -> &str {
        self.get_inner()
    }
}

impl<T: ForIRI> Borrow<str> for IRIMaybe<T> {
    fn borrow(&self) -> &str {
        match self {
            IRIMaybe::Unchecked(t) => t.borrow(),
            IRIMaybe::Validated(iri) => iri.borrow(),
        }
    }
}

impl<T: Deref<Target = str> + ForIRI> Display for IRIMaybe<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.get_inner())
    }
}

impl<T: Deref<Target = str> + ForIRI> PartialEq for IRIMaybe<T> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Unchecked(l0), Self::Unchecked(r0)) => l0 == r0,
            (Self::Validated(l0), Self::Validated(r0)) => l0 == r0,
            (Self::Unchecked(l0), Self::Validated(r0)) => l0.deref() == r0.as_str(),
            (Self::Validated(l0), Self::Unchecked(r0)) => l0.as_str() == r0.deref(),
        }
    }
}

impl<T: Deref<Target = str> + ForIRI> Eq for IRIMaybe<T> {}

impl<T: Deref<Target = str> + ForIRI> Hash for IRIMaybe<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.get_inner().hash(state);
    }
}

impl<T: Deref<Target = str> + ForIRI> Ord for IRIMaybe<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.get_inner().cmp(other.get_inner())
    }
}

impl<T: Deref<Target = str> + ForIRI> PartialOrd for IRIMaybe<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.get_inner().partial_cmp(other.get_inner())
    }
}

impl<T: ForIRI> From<String> for IRIMaybe<T> {
    fn from(s: String) -> Self {
        Self::new_unchecked(s.into())
    }
}

#[cfg(test)]
mod test {
    use std::rc::Rc;

    use super::*;
    use crate::model::*;

    #[test]
    fn building_from_resource_identifier() {
        let b: Build<IRIMaybe<Rc<str>>> = Build::new();

        let _iri = b.iri("http://www.example.com");
        assert!(true)
    }

    #[test]
    fn authority_from_validated_resource_identifier() -> Result<(), Box<dyn std::error::Error>> {
        let b: Build<IRIMaybe<Rc<str>>> = Build::new();

        let iri = b.iri("http://www.example.com");
        let res_identifier = iri.underlying();

        // Builder creates ResourceIdentifier from a String.
        // This amounts to create an unchecked identifier, hence this assertion is successful.
        assert!(!res_identifier.is_validated());

        let validated_identifier = res_identifier.to_validated()?;
        let inner_iri_ref: &oxiri::IriRef<_> = validated_identifier.get_validated().unwrap();

        assert_eq!(inner_iri_ref.authority(), Some("www.example.com"));

        Ok(())
    }
}
