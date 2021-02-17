use std::{io::Write};

use failure::Error;
use rio_xml::{RdfXmlFormatter};
use rio_api::{formatter::TriplesFormatter, model::BlankNode};
use rio_api::model::Triple;

use crate::{model::*,
            ontology::axiom_mapped::AxiomMappedOntology,
            vocab::{OWL, RDF, WithIRI}};

pub fn write<W:Write>(
    write: &mut W,
    ont: &AxiomMappedOntology,
) -> Result<(), Error> {

    let mut f = RdfXmlFormatter::with_indentation(write, 4)?;
    let mut bng = BlankNodeIdGenerator::default();
    ont.render(&mut f, &mut bng)?;
    f.finish()?;
    Ok(())
}

#[derive(Default)]
struct BlankNodeIdGenerator {
    i: u64,
}

impl BlankNodeIdGenerator {
    pub fn generate(&mut self) -> BlankNodeId {
        self.i += 1;
        BlankNodeId(format!{"bn{}", self.i})
    }
}

#[derive(Debug)]
struct BlankNodeId(String);

impl AsRef<str> for BlankNodeId {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}


trait Render {
    fn render<W:Write>(&self, f:&mut RdfXmlFormatter<W>,
                       bng: &mut BlankNodeIdGenerator)-> Result<(), Error>;
}

trait RenderFromBNode {
    fn render_from<W:Write>(&self, f:&mut RdfXmlFormatter<W>,
                            bng: &mut BlankNodeIdGenerator,
                            b: &BlankNode)-> Result<(), Error>;
}

/// The types in `Render` are too long to type.
macro_rules! render {
    ($type:ty, $self:ident, $f:ident, $bng:ident,
     $body:tt) => {
        impl Render for $type {
            fn render<W:Write>(& $self, $f:&mut RdfXmlFormatter<W>,
                               $bng: &mut BlankNodeIdGenerator)-> Result<(), Error>
                $body
        }
    }
}

fn t<'a, NB, NN, T>(subject:NB, predicate:NN, object:T) -> Triple<'a>
where NB: Into<NamedOrBlankNode<'a>>,
      NN: Into<NamedNode<'a>>,
      T: Into<Term<'a>>
{
    Triple{
        subject: subject.into().0,
        predicate: predicate.into().0,
        object: object.into().0,
    }
}

// New types for Rio API terms or we cannot do the generic impls that
// come next
struct NamedOrBlankNode<'a>(rio_api::model::NamedOrBlankNode<'a>);
struct NamedNode<'a>(rio_api::model::NamedNode<'a>);
struct Term<'a>(rio_api::model::Term<'a>);

impl<'a, WI:WithIRI<'a>> From<WI> for NamedNode<'a> {
    fn from(wi: WI) -> Self {
        NamedNode(rio_api::model::NamedNode{iri:wi.iri_str()})
    }
}

impl<'a, WI:WithIRI<'a>> From<WI> for Term<'a> {
    fn from(wi: WI) -> Self {
        let nn:NamedNode = wi.into();
        Term(nn.0.into())
    }
}

impl<'a> From<&'a str> for NamedNode<'a> {
    fn from(iri: &'a str) -> Self {
        NamedNode(rio_api::model::NamedNode{iri})
    }
}

impl<'a> From<&'a str> for NamedOrBlankNode<'a> {
    fn from(iri: &'a str) -> Self {
        let nn:NamedNode = iri.into();
        NamedOrBlankNode(nn.0.into())
    }
}

impl<'a> From<&'a str> for Term<'a> {
    fn from(iri: &'a str) -> Self {
        let nn:NamedNode = iri.into();
        Term(nn.0.into())
    }
}

impl<'a> From<&'a IRI> for NamedNode<'a> {
    fn from(iri: &'a IRI) -> Self {
        iri.as_ref().into()
    }
}

impl<'a> From<&'a IRI> for NamedOrBlankNode<'a> {
    fn from(iri: &'a IRI) -> Self {
        iri.as_ref().into()
    }
}

impl<'a> From<&'a IRI> for Term<'a> {
    fn from(iri: &'a IRI) -> Self {
        iri.as_ref().into()
    }
}


render! {
    &AxiomMappedOntology, self, f, _bng,
    {
        if let Some(iri) = &self.id().iri {
            f.format(&t(
                iri.as_ref(),
                RDF::Type,
                OWL::Ontology))?;
            if let Some(viri) = &self.id().viri {
                f.format(&t(
                    iri.as_ref(),
                    OWL::VersionIRI,
                    viri.as_ref()))?;
            }

            let imp = self.i().import();
            for i in imp {
                f.format(&t(
                    iri.as_ref(),
                    OWL::Imports,
                    &i.0))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {

    extern crate mktemp;

    use self::mktemp::Temp;
    use super::*;
    use crate::{model::Build, ontology::set::SetOntology};

    // use std::collections::HashMap;

    // use std::fs::File;
    use std::{fs::File, io::{BufRead, BufReader, BufWriter}};
    // use std::io::BufReader;
    // use std::io::BufWriter;

    fn read_ok<R: BufRead>(bufread: &mut R) -> SetOntology {
        let r = crate::io::rdf::reader::read(bufread);
        assert!(r.is_ok(), "Expected ontology, got failure:{:?}", r.err());
        let (o, incomplete) = r.ok().unwrap();
        assert!(incomplete.is_complete(), "Read Not Complete: {:#?}", incomplete);
        o.into()
    }

    #[test]
    fn test_ont_rt() {
        let mut ont = AxiomMappedOntology::default();
        let build = Build::new();

        let iri = build.iri("http://www.example.com/a".to_string());
        ont.mut_id().iri = Some(iri);
        let temp_file = Temp::new_file().unwrap();
        let file = File::create(&temp_file).ok().unwrap();
        write(&mut BufWriter::new(file), &ont).ok().unwrap();

        let file = File::open(&temp_file).ok().unwrap();
        let ont2 = read_ok(&mut BufReader::new(file));

        assert_eq!(ont.id().iri, ont2.id().iri);
    }

    fn roundtrip(
        ont: &str,
    ) -> (
        SetOntology,
        SetOntology,
    ) {
        let ont_orig = read_ok(&mut ont.as_bytes());
        let mut temp_file = Temp::new_file().unwrap();

        let file = File::create(&temp_file).ok().unwrap();
        let mut buf_writer = BufWriter::new(&file);

        let amo: AxiomMappedOntology = ont_orig.clone().into();
        write(&mut buf_writer, &amo)
            .ok()
            .unwrap();
        buf_writer.flush().ok();

        let file = File::open(&temp_file).ok().unwrap();

        let ont_round = read_ok(&mut BufReader::new(&file));
        temp_file.release();

        return (ont_orig, ont_round);
    }

    fn assert_round(
        ont: &str,
    ) -> (
        SetOntology,
        SetOntology,
    ) {
        let (ont_orig, ont_round) = roundtrip(ont);

        assert_eq!(ont_orig, ont_round);

        return (ont_orig, ont_round);
    }

    #[test]
    fn round_ont() {
        assert_round(include_str!("../../ont/owl-rdf/ont.owl"));
    }

    // #[test]
    // fn round_one_ont_prefix() {
    //     let (_ont_orig, prefix_orig, _ont_round, prefix_round) =
    //         roundtrip(include_str!("../../ont/owl-rdf/ont.owl"));

    //     let prefix_orig_map: HashMap<&String, &String> = prefix_orig.mappings().collect();

    //     let prefix_round_map: HashMap<&String, &String> = prefix_round.mappings().collect();

    //     assert_eq!(prefix_orig_map, prefix_round_map);
    // }

    // #[test]
    // fn round_class() {
    //     let (ont_orig, ont_round) =
    //         roundtrip(include_str!("../../ont/owl-rdf/class.owl"));

    //     assert_eq!(ont_orig, ont_round);
    // }

    // #[test]
    // fn round_class_with_annotation() {
    //     let (ont_orig, _prefix_orig, ont_round, _prefix_round) = roundtrip(include_str!(
    //         "../../ont/owl-rdf/declaration-with-annotation.owl"
    //     ));

    //     assert_eq!(ont_orig, ont_round);
    // }

    // #[test]
    // fn round_subclass() {
    //     let (ont_orig, _prefix_orig, ont_round, _prefix_round) =
    //         roundtrip(include_str!("../../ont/owl-rdf/subclass.owl"));

    //     assert_eq!(ont_orig, ont_round);
    // }

    // #[test]
    // fn round_oproperty() {
    //     assert_round(include_str!("../../ont/owl-rdf/oproperty.owl"));
    // }

    // #[test]
    // fn round_some() {
    //     assert_round(include_str!("../../ont/owl-rdf/some.owl"));
    // }

    // #[test]
    // fn round_only() {
    //     assert_round(include_str!("../../ont/owl-rdf/only.owl"));
    // }

    // #[test]
    // fn round_and() {
    //     assert_round(include_str!("../../ont/owl-rdf/and.owl"));
    // }

    // #[test]
    // fn round_or() {
    //     assert_round(include_str!("../../ont/owl-rdf/or.owl"));
    // }

    // #[test]
    // fn round_not() {
    //     assert_round(include_str!("../../ont/owl-rdf/not.owl"));
    // }

    // #[test]
    // fn round_annotation_property() {
    //     assert_round(include_str!("../../ont/owl-rdf/annotation-property.owl"));
    // }

    // #[test]
    // fn round_annotation() {
    //     assert_round(include_str!("../../ont/owl-rdf/annotation.owl"));
    // }

    // #[test]
    // fn round_annotation_domain() {
    //     assert_round(include_str!("../../ont/owl-rdf/annotation-domain.owl"));
    // }

    // #[test]
    // fn round_annotation_range() {
    //     assert_round(include_str!("../../ont/owl-rdf/annotation-range.owl"));
    // }

    // #[test]
    // fn round_label() {
    //     assert_round(include_str!("../../ont/owl-rdf/label.owl"));
    // }

    // #[test]
    // fn round_one_comment() {
    //     assert_round(include_str!("../../ont/owl-rdf/one-comment.owl"));
    // }

    // #[test]
    // fn round_one_ontology_annotation() {
    //     assert_round(include_str!(
    //         "../../ont/owl-rdf/one-ontology-annotation.owl"
    //     ));
    // }

    // #[test]
    // fn round_one_equivalent_class() {
    //     assert_round(include_str!("../../ont/owl-rdf/one-equivalent.owl"));
    // }

    // #[test]
    // fn round_one_disjoint_class() {
    //     assert_round(include_str!("../../ont/owl-rdf/one-disjoint.owl"));
    // }

    // #[test]
    // fn round_disjoint_union() {
    //     assert_round(include_str!("../../ont/owl-rdf/disjoint-union.owl"));
    // }

    // #[test]
    // fn round_one_sub_property() {
    //     assert_round(include_str!("../../ont/owl-rdf/one-suboproperty.owl"));
    // }

    // #[test]
    // fn round_one_inverse() {
    //     assert_round(include_str!("../../ont/owl-rdf/inverse-properties.owl"));
    // }

    // #[test]
    // fn round_one_transitive() {
    //     assert_round(include_str!("../../ont/owl-rdf/transitive-properties.owl"));
    // }

    // #[test]
    // fn round_one_annotated_transitive() {
    //     assert_round(include_str!(
    //         "../../ont/owl-rdf/annotation-on-transitive.owl"
    //     ));
    // }

    // #[test]
    // fn round_one_subproperty_chain() {
    //     assert_round(include_str!("../../ont/owl-rdf/subproperty-chain.owl"));
    // }

    // #[test]
    // fn round_one_subproperty_chain_with_inverse() {
    //     assert_round(include_str!(
    //         "../../ont/owl-rdf/subproperty-chain-with-inverse.owl"
    //     ));
    // }

    // #[test]
    // fn round_annotation_on_annotation() {
    //     assert_round(include_str!(
    //         "../../ont/owl-rdf/annotation-with-annotation.owl"
    //     ));
    // }

    // #[test]
    // fn round_sub_annotation() {
    //     assert_round(include_str!("../../ont/owl-rdf/sub-annotation.owl"));
    // }

    // #[test]
    // fn round_data_property() {
    //     assert_round(include_str!("../../ont/owl-rdf/data-property.owl"));
    // }

    // #[test]
    // fn round_literal_escaped() {
    //     assert_round(include_str!("../../ont/owl-rdf/literal-escaped.owl"));
    // }

    // #[test]
    // fn round_named_individual() {
    //     assert_round(include_str!("../../ont/owl-rdf/named-individual.owl"));
    // }

    #[test]
    fn round_import() {
        assert_round(include_str!("../../ont/owl-rdf/import.owl"));
    }

    // #[test]
    // fn datatype() {
    //     assert_round(include_str!("../../ont/owl-rdf/datatype.owl"));
    // }

    // #[test]
    // fn object_has_value() {
    //     assert_round(include_str!("../../ont/owl-rdf/object-has-value.owl"));
    // }

    // #[test]
    // fn object_one_of() {
    //     assert_round(include_str!("../../ont/owl-rdf/object-one-of.owl"));
    // }

    // #[test]
    // fn inverse() {
    //     assert_round(include_str!("../../ont/owl-rdf/some-inverse.owl"));
    // }

    // #[test]
    // fn object_unqualified_cardinality() {
    //     assert_round(include_str!(
    //         "../../ont/owl-rdf/object-unqualified-max-cardinality.owl"
    //     ));
    // }

    // #[test]
    // fn object_min_cardinality() {
    //     assert_round(include_str!("../../ont/owl-rdf/object-min-cardinality.owl"));
    // }

    // #[test]
    // fn object_max_cardinality() {
    //     assert_round(include_str!("../../ont/owl-rdf/object-max-cardinality.owl"));
    // }

    // #[test]
    // fn object_exact_cardinality() {
    //     assert_round(include_str!(
    //         "../../ont/owl-rdf/object-exact-cardinality.owl"
    //     ));
    // }

    // #[test]
    // fn datatype_alias() {
    //     assert_round(include_str!("../../ont/owl-rdf/datatype-alias.owl"));
    // }

    // #[test]
    // fn datatype_intersection() {
    //     assert_round(include_str!("../../ont/owl-rdf/datatype-intersection.owl"));
    // }

    // #[test]
    // fn datatype_union() {
    //     assert_round(include_str!("../../ont/owl-rdf/datatype-union.owl"));
    // }

    // #[test]
    // fn datatype_complement() {
    //     assert_round(include_str!("../../ont/owl-rdf/datatype-complement.owl"));
    // }

    // #[test]
    // fn datatype_oneof() {
    //     assert_round(include_str!("../../ont/owl-rdf/datatype-oneof.owl"));
    // }

    // #[test]
    // fn datatype_some() {
    //     assert_round(include_str!("../../ont/owl-rdf/data-some.owl"));
    // }

    // #[test]
    // fn facet_restriction() {
    //     assert_round(include_str!("../../ont/owl-rdf/facet-restriction.owl"));
    // }

    // #[test]
    // fn data_only() {
    //     assert_round(include_str!("../../ont/owl-rdf/data-only.owl"));
    // }
    // #[test]
    // fn data_exact_cardinality() {
    //     assert_round(include_str!("../../ont/owl-rdf/data-exact-cardinality.owl"));
    // }

    // #[test]
    // fn data_has_value() {
    //     assert_round(include_str!("../../ont/owl-rdf/data-has-value.owl"));
    // }

    // #[test]
    // fn data_max_cardinality() {
    //     assert_round(include_str!("../../ont/owl-rdf/data-max-cardinality.owl"));
    // }

    // #[test]
    // fn data_min_cardinality() {
    //     assert_round(include_str!("../../ont/owl-rdf/data-min-cardinality.owl"));
    // }

    // #[test]
    // fn class_assertion() {
    //     assert_round(include_str!("../../ont/owl-rdf/class-assertion.owl"));
    // }

    // #[test]
    // fn data_property_assertion() {
    //     assert_round(include_str!(
    //         "../../ont/owl-rdf/data-property-assertion.owl"
    //     ));
    // }

    // #[test]
    // fn same_individual() {
    //     assert_round(include_str!("../../ont/owl-rdf/same-individual.owl"));
    // }

    // #[test]
    // fn different_individuals() {
    //     assert_round(include_str!("../../ont/owl-rdf/different-individual.owl"));
    // }

    // #[test]
    // fn negative_data_property_assertion() {
    //     assert_round(include_str!(
    //         "../../ont/owl-rdf/negative-data-property-assertion.owl"
    //     ));
    // }

    // #[test]
    // fn negative_object_property_assertion() {
    //     assert_round(include_str!(
    //         "../../ont/owl-rdf/negative-object-property-assertion.owl"
    //     ));
    // }

    // #[test]
    // fn object_property_assertion() {
    //     assert_round(include_str!(
    //         "../../ont/owl-rdf/object-property-assertion.owl"
    //     ));
    // }

    // #[test]
    // fn data_has_key() {
    //     assert_round(include_str!("../../ont/owl-rdf/data-has-key.owl"));
    // }

    // #[test]
    // fn data_property_disjoint() {
    //     assert_round(include_str!("../../ont/owl-rdf/data-property-disjoint.owl"));
    // }

    // #[test]
    // fn data_property_domain() {
    //     assert_round(include_str!("../../ont/owl-rdf/data-property-domain.owl"));
    // }

    // #[test]
    // fn data_property_equivalent() {
    //     assert_round(include_str!(
    //         "../../ont/owl-rdf/data-property-equivalent.owl"
    //     ));
    // }

    // #[test]
    // fn data_property_functional() {
    //     assert_round(include_str!(
    //         "../../ont/owl-rdf/data-property-functional.owl"
    //     ));
    // }

    // #[test]
    // fn data_property_range() {
    //     assert_round(include_str!("../../ont/owl-rdf/data-property-range.owl"));
    // }

    // #[test]
    // fn data_property_sub() {
    //     assert_round(include_str!("../../ont/owl-rdf/data-property-sub.owl"));
    // }

    // #[test]
    // fn disjoint_object_properties() {
    //     assert_round(include_str!(
    //         "../../ont/owl-rdf/disjoint-object-properties.owl"
    //     ));
    // }

    // #[test]
    // fn equivalent_object_properties() {
    //     assert_round(include_str!(
    //         "../../ont/owl-rdf/equivalent-object-properties.owl"
    //     ));
    // }

    // #[test]
    // fn object_has_key() {
    //     assert_round(include_str!("../../ont/owl-rdf/object-has-key.owl"));
    // }

    // #[test]
    // fn object_property_asymmetric() {
    //     assert_round(include_str!(
    //         "../../ont/owl-rdf/object-property-asymmetric.owl"
    //     ));
    // }

    // #[test]
    // fn object_property_domain() {
    //     assert_round(include_str!("../../ont/owl-rdf/object-property-domain.owl"));
    // }

    // #[test]
    // fn object_property_functional() {
    //     assert_round(include_str!(
    //         "../../ont/owl-rdf/object-property-functional.owl"
    //     ));
    // }

    // #[test]
    // fn object_property_inverse_functional() {
    //     assert_round(include_str!(
    //         "../../ont/owl-rdf/object-property-inverse-functional.owl"
    //     ));
    // }

    // #[test]
    // fn object_property_irreflexive() {
    //     assert_round(include_str!(
    //         "../../ont/owl-rdf/object-property-irreflexive.owl"
    //     ));
    // }

    // #[test]
    // fn object_property_range() {
    //     assert_round(include_str!("../../ont/owl-rdf/object-property-range.owl"));
    // }

    // #[test]
    // fn object_property_reflexive() {
    //     assert_round(include_str!(
    //         "../../ont/owl-rdf/object-property-reflexive.owl"
    //     ));
    // }

    // #[test]
    // fn object_property_symmetric() {
    //     assert_round(include_str!(
    //         "../../ont/owl-rdf/object-property-symmetric.owl"
    //     ));
    // }

    // #[test]
    // fn family() {
    //     assert_round(include_str!("../../ont/owl-rdf/family.owl"));
    // }

}