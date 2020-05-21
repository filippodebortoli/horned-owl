#![allow(unused_imports)]
use Term::*;

use curie::PrefixMapping;

use crate::index::find_declaration_kind;
use crate::index::is_annotation_property;
use crate::index::update_logically_equal_axiom;
use crate::model::Literal;
use crate::model::*;
use crate::vocab::WithIRI;
use crate::vocab::OWL as VOWL;
use crate::vocab::RDF as VRDF;
use crate::vocab::RDFS as VRDFS;

use enum_meta::Meta;
use failure::Error;
use failure::SyncFailure;

use log::{debug, trace};

use sophia::term::BNodeId;
use sophia::term::IriData;
use sophia::term::LiteralKind;

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::io::BufRead;
use std::rc::Rc;

// Two type aliases for "SoPhia" entities.
type SpTerm = sophia::term::Term<Rc<str>>;
type SpIri = IriData<Rc<str>>;
type SpBNode = BNodeId<Rc<str>>;

macro_rules! some {
    ($body:expr) => {
        (|| Some($body))()
    };
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum Term {
    Iri(IRI),
    BNode(SpBNode),
    Literal(Rc<str>, LiteralKind<Rc<str>>),
    Variable(Rc<str>),
    OWL(VOWL),
    RDF(VRDF),
    RDFS(VRDFS),
}

impl Term {
    fn ord(&self) -> isize {
        match self {
            OWL(_) => 1,
            RDF(_) => 2,
            RDFS(_) => 3,
            Iri(_) => 4,
            BNode(_) => 5,
            Literal(_, _) => 6,
            Variable(_) => 7,
        }
    }
}

// impl PartialEq for Term {
//     fn eq(&self, other: &Self) -> bool {
//         match
//     }
// }

impl Ord for Term {
    fn cmp(&self, other: &Term) -> Ordering {
        match (self, other) {
            (OWL(s), OWL(o)) => s.cmp(o),
            (RDF(s), RDF(o)) => s.cmp(o),
            (RDFS(s), RDFS(o)) => s.cmp(o),
            (Iri(s), Iri(o)) => s.to_string().cmp(&o.to_string()),
            (BNode(s), BNode(o)) => (*s).cmp(&(*o)),
            (Literal(s, _), Literal(o, _)) => s.cmp(o),
            (Variable(s), Variable(o)) => s.cmp(o),
            _ => self.ord().cmp(&other.ord()),
        }
    }
}

impl PartialOrd for Term {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// impl Term {
//     pub fn n3_maybe(&self) -> String {
//         match self {
//             Iri(_) | BNode(_) | Literal(_, _) | Variable(_) => self.n3(),
//             OWL(v) => format!("{:?}", v),
//             RDFS(v) => format!("{:?}", v),
//             RDF(v) => format!("{:?}", v),
//         }
//     }

//     pub fn n3(&self) -> String {
//         match self {
//             Iri(i) => sophia::term::Term::Iri(i.clone()).n3(),
//             BNode(id) => sophia::term::Term::BNode(id.clone()).n3(),
//             Literal(l, k) => sophia::term::Term::Literal(l.clone(), k.clone()).n3(),
//             Variable(v) => sophia::term::Term::Variable(v.clone()).n3(),
//             OWL(v) => vocab_to_term(v).n3(),
//             RDFS(v) => vocab_to_term(v).n3(),
//             RDF(v) => vocab_to_term(v).n3(),
//         }
//     }

//     pub fn value(&self) -> String {
//         match self {
//             Iri(i) => sophia::term::Term::Iri(i.clone()).value(),
//             BNode(id) => sophia::term::Term::BNode(id.clone()).value(),
//             Literal(l, k) => sophia::term::Term::Literal(l.clone(), k.clone()).value(),
//             Variable(v) => sophia::term::Term::Variable(v.clone()).value(),
//             OWL(v) => vocab_to_term(v).value(),
//             RDFS(v) => vocab_to_term(v).value(),
//             RDF(v) => vocab_to_term(v).value(),
//         }
//     }
// }

trait Convert {
    fn to_iri(&self, b: &Build) -> IRI;
}

impl Convert for SpIri {
    fn to_iri(&self, b: &Build) -> IRI {
        b.iri(self.to_string())
    }
}

trait TryBuild<N: From<IRI>> {
    fn to_some_iri(&self, b: &Build) -> Option<IRI>;

    fn to_iri_maybe(&self, b: &Build) -> Result<IRI, Error> {
        match self.to_some_iri(b) {
            Some(iri) => Ok(iri),
            None => todo!("Fix this"),
        }
    }

    fn try_build(&self, b: &Build) -> Result<N, Error> {
        Ok(self.to_iri_maybe(b)?.into())
    }
}

impl<N: From<IRI>> TryBuild<N> for Option<SpIri> {
    fn to_some_iri(&self, b: &Build) -> Option<IRI> {
        self.as_ref().map(|i| i.to_iri(b))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum OrTerm {
    Term(Term),
    ClassExpression(ClassExpression),
}

impl From<ClassExpression> for OrTerm {
    fn from(c: ClassExpression) -> OrTerm {
        OrTerm::ClassExpression(c)
    }
}

impl From<Term> for OrTerm {
    fn from(t: Term) -> OrTerm {
        OrTerm::Term(t)
    }
}

fn vocab_to_term<'a, V: WithIRI<'a>>(v: &V) -> SpTerm {
    // unwrap should be safe for all known WithIRIs
    sophia::term::Term::new_iri(Rc::from(v.iri_str())).unwrap()
}

fn vocab_lookup() -> HashMap<SpTerm, Term> {
    let mut m = HashMap::default();

    for v in VOWL::all() {
        m.insert(vocab_to_term(&v), Term::OWL(v));
    }

    for v in VRDFS::all() {
        m.insert(vocab_to_term(&v), Term::RDFS(v));
    }

    for v in VRDF::all() {
        m.insert(vocab_to_term(&v), Term::RDF(v));
    }

    m
}

fn to_term(t: &SpTerm, m: &HashMap<SpTerm, Term>, b: &Build) -> Term {
    if let Some(t) = m.get(t) {
        t.clone()
    } else {
        match t {
            sophia::term::Term::Iri(i) => Iri(i.to_iri(b)),
            sophia::term::Term::BNode(id) => BNode(id.clone()),
            sophia::term::Term::Literal(l, k) => Literal(l.clone(), k.clone()),
            sophia::term::Term::Variable(v) => Variable(v.clone()),
        }
    }
}

macro_rules! d {
    () => {
        Default::default()
    };
}

struct OntologyParser<'a> {
    o: Ontology,
    b: &'a Build,

    simple: Vec<[Term;3]>,
    bnode: HashMap<SpBNode, Vec<[Term; 3]>>,
    bnode_seq: HashMap<SpBNode, Vec<Term>>,

    class_expression: HashMap<SpBNode, ClassExpression>,
    object_property_expression: HashMap<SpBNode, ObjectPropertyExpression>,
    data_range: HashMap<SpBNode, DataRange>,
    ann_map: HashMap<[Term; 3], BTreeSet<Annotation>>,
}

impl<'a> OntologyParser<'a> {
    fn new(b: &'a Build) -> OntologyParser {
        OntologyParser {
            o: d!(),
            b,

            simple: d!(),
            bnode: d!(),
            bnode_seq: d!(),
            class_expression: d!(),
            object_property_expression: d!(),
            data_range: d!(),
            ann_map: d!(),
        }
    }

    fn group_triples(
        triple: Vec<[Term; 3]>,
        simple: &mut Vec<[Term; 3]>,
        bnode: &mut HashMap<SpBNode, Vec<[Term; 3]>>,
    ) {
        // Next group together triples on a BNode, so we have
        // HashMap<BNodeID, Vec<[SpTerm; 3]> All of which should be
        // triples should begin with the BNodeId. We should be able to
        // gather these in a single pass.
        for t in &triple {
            match t {
                [BNode(id), _, _] => {
                    let v = bnode.entry(id.clone()).or_insert_with(Vec::new);
                    v.push(t.clone())
                }
                _ => {
                    simple.push(t.clone());
                }
            }
        }
    }

    fn stitch_seqs_1(
        &mut self,
    ) {
        let mut extended = false;

        for (k, v) in std::mem::take(&mut self.bnode) {
            #[rustfmt::skip]
            let _ = match v.as_slice() {
                [[_, Term::RDF(VRDF::First), val],
                 [_, Term::RDF(VRDF::Rest), Term::BNode(bnode_id)]] => {
                    let some_seq = self.bnode_seq.remove(bnode_id);
                    if let Some(mut seq) = some_seq {
                        seq.push(val.clone());
                        self.bnode_seq.insert(k.clone(), seq);
                        extended = true;
                    } else {
                        self.bnode.insert(k, v);
                    }
                }
                _ => {
                    self.bnode.insert(k, v);
                }
            };
        }

        if extended && self.bnode.len() > 0 {
            self.stitch_seqs_1()
        }
    }

    fn stitch_seqs(
        &mut self,
    ) {
        for (k, v) in std::mem::take(&mut self.bnode) {
            #[rustfmt::skip]
            let _ = match v.as_slice() {
                [[_, Term::RDF(VRDF::First), val],
                 [_, Term::RDF(VRDF::Rest), Term::RDF(VRDF::Nil)]] =>
                {
                    self.bnode_seq.insert(k.clone(), vec![val.clone()]);
                }
                _ => {
                    self.bnode.insert(k, v);
                }
            };
        }

        self.stitch_seqs_1();

        for (_, v) in self.bnode_seq.iter_mut() {
            v.reverse();
        }
    }

    fn resolve_imports(&mut self) {
        // Section 3.1.2/table 4 of RDF Graphs
    }

    fn headers(&mut self) {
        //Section 3.1.2/table 4
        //   *:x rdf:type owl:Ontology .
        //[ *:x owl:versionIRI *:y .]
        let mut iri: Option<IRI> = None;
        let mut viri: Option<IRI> = None;

        for t in std::mem::take(&mut self.simple) {
            match t {
                [Term::Iri(s), Term::RDF(VRDF::Type), Term::OWL(VOWL::Ontology)] => {
                    iri = Some(s.clone());
                }
                [Term::Iri(s), Term::OWL(VOWL::VersionIRI), Term::Iri(ob)]
                    if iri.as_ref() == Some(&s) =>
                {
                    viri = Some(ob.clone());
                }
                _ => self.simple.push(t.clone()),
            }
        }

        self.o.id.iri = iri;
        self.o.id.viri = viri;
    }

    fn backward_compat(&mut self) {
        // Table 5, Table 6
    }

    fn parse_annotations(&self, triples: &[[Term; 3]]) -> BTreeSet<Annotation> {
        let mut ann = BTreeSet::default();
        for a in triples {
            ann.insert(self.annotation(a));
        }
        ann
    }

    fn annotation(&self, t: &[Term; 3]) -> Annotation {
        match t {
            // We assume that anything passed to here is an
            // annotation built in type
            [s, RDFS(rdfs), b] => {
                let iri = self.b.iri(rdfs.iri_s());
                self.annotation(&[s.clone(), Term::Iri(iri), b.clone()])
            }
            [_, Iri(p), ob @ Term::Literal(_, _)] => Annotation {
                ap: AnnotationProperty(p.clone()),
                av: self.to_literal(ob).unwrap().into(),
            },
            [_, Iri(p), Iri(ob)] => {
                // IRI annotation value
                Annotation {
                    ap: AnnotationProperty(p.clone()),
                    av: ob.clone().into(),
                }
            }
            _ => todo!(),
        }
    }

    fn merge<A: Into<AnnotatedAxiom>>(&mut self, ax: A) {
        let ax = ax.into();
        update_logically_equal_axiom(&mut self.o, ax);
    }

    fn in_or_merge<A: Into<AnnotatedAxiom>>(&mut self, ax: A) {
        let ax = ax.into();
        let is_empty = ax.ann.is_empty();

        if is_empty {
            self.o.insert(ax);
        } else {
            update_logically_equal_axiom(&mut self.o, ax);
        }
    }

    fn axiom_annotations(
        &mut self,
    ) {
        for (k, v) in std::mem::take(&mut self.bnode) {
            match v.as_slice() {
                #[rustfmt::skip]
                [[_, Term::OWL(VOWL::AnnotatedProperty), p],
                 [_, Term::OWL(VOWL::AnnotatedSource), sb],
                 [_, Term::OWL(VOWL::AnnotatedTarget), ob],
                 [_, Term::RDF(VRDF::Type), Term::OWL(VOWL::Axiom)],
                 ann @ ..] =>
                {
                    self.ann_map.insert(
                        [sb.clone(), p.clone(), ob.clone()],
                        self.parse_annotations(ann),
                    );
                    self.simple.push([sb.clone(), p.clone(), ob.clone()])
                }

                _ => {
                    self.bnode.insert(k, v);
                }
            }
        }
    }

    fn declarations(&mut self) {
        // Table 7
        for triple in std::mem::take(&mut self.simple) {
            let entity = match &triple {
                // TODO Change this into a single outer match
                [Term::Iri(s), Term::RDF(VRDF::Type), entity] => {
                    // TODO Move match into function
                    match entity {
                        Term::OWL(VOWL::Class) => Some(Class(s.clone()).into()),
                        Term::OWL(VOWL::ObjectProperty) => Some(ObjectProperty(s.clone()).into()),
                        Term::OWL(VOWL::AnnotationProperty) => {
                            Some(AnnotationProperty(s.clone()).into())
                        }
                        Term::OWL(VOWL::DatatypeProperty) => Some(DataProperty(s.clone()).into()),
                        Term::OWL(VOWL::NamedIndividual) => Some(NamedIndividual(s.clone()).into()),
                        Term::RDFS(VRDFS::Datatype) => Some(Datatype(s.clone()).into()),
                        _ => None,
                    }
                }
                _ => None,
            };

            if let Some(entity) = entity {
                let ann = self.ann_map.remove(&triple).unwrap_or_else(|| BTreeSet::new());
                self.merge(AnnotatedAxiom {
                    axiom: declaration(entity),
                    ann
                });
            } else {
                self.simple.push(triple);
            }
        }
    }

    fn data_ranges(&mut self) {
        let data_range_len = self.data_range.len();
        for (this_bnode, v) in std::mem::take(&mut self.bnode) {
            let dr = match v.as_slice()  {
                [[_, Term::OWL(VOWL::IntersectionOf), Term::BNode(bnodeid)],
                 [_, Term::RDF(VRDF::Type), Term::RDFS(VRDFS::Datatype)]] => {
                    some!{
                        DataRange::DataIntersectionOf(
                            self.to_dr_seq(bnodeid)?
                        )
                    }
                },
                [[_, Term::OWL(VOWL::DatatypeComplementOf), term],
                  [_, Term::RDF(VRDF::Type), Term::RDFS(VRDFS::Datatype)]] => {
                     some!{
                       DataRange::DataComplementOf(
                             Box::new(self.to_dr(term)?)
                         )
                     }
                 },
                _ => None
            };

            if let Some(dr) = dr {
                self.data_range.insert(this_bnode, dr);
            }
            else {
                self.bnode.insert(this_bnode, v);
            }
        }

        if self.data_range.len() > data_range_len {
            self.data_ranges();
        }
    }

    fn object_property_expressions(
        &mut self,
    )  {
        for (this_bnode, v) in std::mem::take(&mut self.bnode) {
            let mut ope = None;
            let mut new_triple = vec![];
            for t in v {
                match t {
                    [Term::BNode(_), Term::OWL(VOWL::InverseOf), Term::Iri(iri)] => {
                        ope = Some(ObjectPropertyExpression::InverseObjectProperty(iri.into()))
                    }
                    _ => {
                        new_triple.push(t);
                    }
                };
            }

            if let Some(ope) = ope {
                self.object_property_expression.insert(this_bnode.clone(), ope);
            }

            if new_triple.len() > 0 {
                self.bnode.insert(this_bnode, new_triple);
            }
        }
    }

    fn to_iri(&self, t: &Term) -> Option<IRI> {
        match t {
            Term::Iri(iri) => Some(iri.clone()),
            _ => None,
        }
    }

    fn to_sope(
        &mut self,
        t: &Term,
    ) -> Option<SubObjectPropertyExpression> {
        Some(self.to_ope(t)?.into())
    }

    fn to_ope(
        &mut self,
        t: &Term,
    ) -> Option<ObjectPropertyExpression> {
        match self.find_property_kind(t)? {
            PropertyExpression::ObjectPropertyExpression(ope) => Some(ope),
            _ => None,
        }
    }

    fn to_ap(&mut self, t: &Term) -> Option<AnnotationProperty> {
        match self.find_property_kind(t)? {
            PropertyExpression::AnnotationProperty(ap) => Some(ap),
            _ => None,
        }
    }

    fn to_ce(
        &mut self,
        tce: &Term,
    ) -> Option<ClassExpression> {
        match tce {
            Term::Iri(cl) => Some(Class(cl.clone()).into()),
            Term::BNode(id) => self.class_expression.remove(id),
            _ => None,
        }
    }

    fn to_ce_seq(
        &mut self,
        bnodeid: &SpBNode,
    ) -> Option<Vec<ClassExpression>> {
        let v: Vec<Option<ClassExpression>> = self.bnode_seq
            .remove(bnodeid)
            .as_ref()?
            .into_iter()
            .map(|tce| self.to_ce(tce))
            .collect();

        // All or nothing
        v.into_iter().collect()
    }

    fn to_ni_seq(&mut self, bnodeid: &SpBNode) -> Option<Vec<NamedIndividual>> {
        let v: Vec<Option<NamedIndividual>> = self.bnode_seq
            .remove(bnodeid)
            .as_ref()?
            .into_iter()
            .map(|t| self.to_iri(t).map(|iri| NamedIndividual(iri.clone())))
            .collect();

        v.into_iter().collect()
    }

    fn to_dr_seq(&mut self, bnodeid: &SpBNode) -> Option<Vec<DataRange>>{
        let v: Vec<Option<DataRange>> = self.bnode_seq
            .remove(bnodeid)
            .as_ref()?
            .into_iter()
            .map(|t| self.to_dr(t))
            .collect();

        v.into_iter().collect()
    }

    fn to_dr(&mut self, t: &Term) -> Option<DataRange> {
        match t {
            Term::Iri(iri) => {
                let dt: Datatype = iri.into();
                Some(dt.into())
            }
            Term::BNode(id) => {
                self.data_range.remove(id)
            }
            _ => todo!(),
        }
    }

    fn to_u32(&self, t: &Term) -> Option<u32> {
        match t {
            Term::Literal(val, LiteralKind::Datatype(_)) => val.parse::<u32>().ok(),
            _ => None,
        }
    }

    fn to_literal(&self, t: &Term) -> Option<Literal> {
        Some(match t {
            Term::Literal(ob, LiteralKind::Lang(lang)) => Literal::Language {
                lang: lang.clone().to_string(),
                literal: ob.clone().to_string(),
            },
            Term::Literal(ob, LiteralKind::Datatype(iri))
                if iri.to_string() == "http://www.w3.org/2001/XMLSchema#string" =>
            {
                Literal::Simple {
                    literal: ob.clone().to_string(),
                }
            }
            Term::Literal(ob, LiteralKind::Datatype(iri)) => Literal::Datatype {
                datatype_iri: iri.to_iri(self.b),
                literal: ob.clone().to_string(),
            },
            _ => return None,
        })
    }

    fn find_property_kind(
        &mut self,
        term: &Term,
    ) -> Option<PropertyExpression> {
        match term {
            Term::Iri(iri) => match find_declaration_kind(&self.o, iri) {
                Some(NamedEntityKind::AnnotationProperty) => {
                    Some(PropertyExpression::AnnotationProperty(iri.into()))
                }
                Some(NamedEntityKind::DataProperty) => {
                    Some(PropertyExpression::DataProperty(iri.into()))
                }
                Some(NamedEntityKind::ObjectProperty) => {
                    Some(PropertyExpression::ObjectPropertyExpression(iri.into()))
                }
                _ => None,
            },
            Term::BNode(id) => Some(self.object_property_expression.remove(id)?.into()),
            _ => None,
        }
    }

    fn class_expressions(
        &mut self,
    ) {

        let class_expression_len = self.class_expression.len();
        for (this_bnode, v) in std::mem::take(&mut self.bnode) {
            // rustfmt breaks this (putting the triples all on one
            // line) so skip
            #[rustfmt::skip]
            let ce = match v.as_slice() {
                [[_, Term::OWL(VOWL::OnProperty), pr],
                 [_, Term::OWL(VOWL::SomeValuesFrom), ce_or_dr],
                 [_, Term::RDF(VRDF::Type), Term::OWL(VOWL::Restriction)]] => {
                    some! {
                        match self.find_property_kind(pr)? {
                            PropertyExpression::ObjectPropertyExpression(ope) => {
                                ClassExpression::ObjectSomeValuesFrom {
                                    ope,
                                    bce: self.to_ce(ce_or_dr)?.into()
                                }
                            },
                            PropertyExpression::DataProperty(dp) => {
                                ClassExpression::DataSomeValuesFrom {
                                    dp,
                                    dr: self.to_dr(ce_or_dr)?
                                }
                            },
                            _ => panic!("Unexpected Property Kind")
                        }
                    }
                },
                [[_, Term::OWL(VOWL::HasValue), val],
                 [_, Term::OWL(VOWL::OnProperty), pr],
                 [_, Term::RDF(VRDF::Type), Term::OWL(VOWL::Restriction)]] => {
                    some! {
                        match self.find_property_kind(pr)? {
                            PropertyExpression::ObjectPropertyExpression(ope) => {
                                ClassExpression::ObjectHasValue {
                                    ope,
                                    i: NamedIndividual(self.to_iri(val)?).into()
                                }
                            },
                            PropertyExpression::DataProperty(dp) => {
                                ClassExpression::DataHasValue {
                                    dp,
                                    l: self.to_literal(val)?
                                }
                            }
                            _ => panic!("Unexpected Property kind"),
                        }
                    }
                },
                [[_, Term::OWL(VOWL::AllValuesFrom), ce_or_dr],
                 [_, Term::OWL(VOWL::OnProperty), pr],
                 [_, Term::RDF(VRDF::Type), Term::OWL(VOWL::Restriction)]] => {
                    some! {
                        match self.find_property_kind(pr)? {
                            PropertyExpression::ObjectPropertyExpression(ope) => {
                                ClassExpression::ObjectAllValuesFrom {
                                    ope,
                                    bce: self.to_ce(ce_or_dr)?.into()
                                }
                            },
                            PropertyExpression::DataProperty(dp) => {
                                ClassExpression::DataAllValuesFrom {
                                    dp,
                                    dr: self.to_dr(ce_or_dr)?
                                }
                            },
                            _ => panic!("Unexpected Property Kind")
                        }
                    }
                },
                [[_, Term::OWL(VOWL::OneOf), Term::BNode(bnodeid)],
                 [_, Term::RDF(VRDF::Type), Term::OWL(VOWL::Class)]] => {
                    some!{
                        ClassExpression::ObjectOneOf(
                            self.to_ni_seq(bnodeid)?
                        )
                    }
                },
                [[_, Term::OWL(VOWL::IntersectionOf), Term::BNode(bnodeid)],
                 [_, Term::RDF(VRDF::Type), Term::OWL(VOWL::Class)]] => {
                    some!{
                        ClassExpression::ObjectIntersectionOf(
                            self.to_ce_seq(bnodeid)?
                        )
                    }
                },
                [[_, Term::OWL(VOWL::UnionOf), Term::BNode(bnodeid)],
                 [_, Term::RDF(VRDF::Type), Term::OWL(VOWL::Class)]] => {
                    some!{
                        ClassExpression::ObjectUnionOf(
                            self.to_ce_seq(
                                bnodeid,
                            )?
                        )
                    }
                },
                [[_, Term::OWL(VOWL::ComplementOf), tce],
                 [_, Term::RDF(VRDF::Type), Term::OWL(VOWL::Class)]] => {
                    some!{
                        ClassExpression::ObjectComplementOf(
                            self.to_ce(&tce)?.into()
                        )
                    }
                },
                [[_, Term::OWL(VOWL::OnDataRange), dr],
                 [_, Term::OWL(VOWL::OnProperty), Term::Iri(pr)],
                 [_, Term::OWL(VOWL::QualifiedCardinality), literal],
                 [_, Term::RDF(VRDF::Type), Term::OWL(VOWL::Restriction)]
                ] => {
                    some!{
                        ClassExpression::DataExactCardinality
                        {
                            n:self.to_u32(literal)?,
                            dp: pr.into(),
                            dr: self.to_dr(dr)?
                        }
                    }
                }

                [[_, Term::OWL(VOWL::OnClass), tce],
                 [_, Term::OWL(VOWL::OnProperty), Term::Iri(pr)],
                 [_, Term::OWL(VOWL::QualifiedCardinality), literal],
                 [_, Term::RDF(VRDF::Type), Term::OWL(VOWL::Restriction)]
                ] => {
                    some!{
                        ClassExpression::ObjectExactCardinality
                        {
                            n:self.to_u32(literal)?,
                            ope: pr.into(),
                            bce: self.to_ce(tce)?.into()
                        }
                    }
                }
                [[_, Term::OWL(VOWL::MinQualifiedCardinality), literal],
                 [_, Term::OWL(VOWL::OnClass), tce],
                 [_, Term::OWL(VOWL::OnProperty), Term::Iri(pr)],
                 [_, Term::RDF(VRDF::Type), Term::OWL(VOWL::Restriction)]
                ] => {
                    some!{
                        ClassExpression::ObjectMinCardinality
                        {
                            n:self.to_u32(literal)?,
                            ope: pr.into(),
                            bce: self.to_ce(tce)?.into()
                        }
                    }
                }
                [[_, Term::OWL(VOWL::MaxQualifiedCardinality), literal],
                 [_, Term::OWL(VOWL::OnClass), tce],
                 [_, Term::OWL(VOWL::OnProperty), Term::Iri(pr)],
                 [_, Term::RDF(VRDF::Type), Term::OWL(VOWL::Restriction)]
                ] => {
                    some!{
                        ClassExpression::ObjectMaxCardinality
                        {
                            n:self.to_u32(literal)?,
                            ope: pr.into(),
                            bce: self.to_ce(tce)?.into()
                        }
                    }
                }
                [[_, Term::OWL(VOWL::MaxCardinality), literal],
                 [_, Term::OWL(VOWL::OnProperty), Term::Iri(pr)],
                 [_, Term::RDF(VRDF::Type), Term::OWL(VOWL::Restriction)]
                ] => {
                    some!{
                        ClassExpression::ObjectMaxCardinality
                        {
                            n:self.to_u32(literal)?,
                            ope: pr.into(),
                            bce: self.b.class(VOWL::Thing.iri_s().to_string()).into()
                        }
                    }
                }

                _a => None,
            };

            if let Some(ce) = ce {
                self.class_expression.insert(this_bnode, ce);
            } else {
                self.bnode.insert(this_bnode, v);
            }
        }

        if self.class_expression.len() > class_expression_len {
            self.class_expressions()
        }
    }

    fn axioms(
        &mut self,
    ) {
        for triple in std::mem::take(&mut self.simple)
            .into_iter()
            .chain(std::mem::take(&mut self.bnode).into_iter().map(|(_k, v)| v).flatten())
        {
            let axiom: Option<Axiom> = match &triple {
                [Term::Iri(sub), Term::RDFS(VRDFS::SubClassOf), tce] => some! {
                    SubClassOf {
                        sub: Class(sub.clone()).into(),
                        sup: self.to_ce(tce)?,
                    }
                    .into()
                },
                // TODO: We need to check whether these
                // EquivalentClasses have any other EquivalentClasses
                // and add to that axiom
                [Term::Iri(a), Term::OWL(VOWL::EquivalentClass), b] => {
                    some!{
                        match find_declaration_kind(&self.o, a)? {
                            NamedEntityKind::Class => {
                                EquivalentClasses(vec![
                                    // The order is not important here, but
                                    // this way around matches with the XML reader
                                    self.to_ce(b)?,
                                    Class(a.clone()).into(),
                                ]).into()
                            }
                            NamedEntityKind::Datatype => {
                                DatatypeDefinition{
                                    kind: Datatype(a.clone()).into(),
                                    range: self.to_dr(b)?,
                                }.into()
                            }
                            _=> todo!()
                        }
                    }
                }
                [Term::Iri(iri), Term::OWL(VOWL::DisjointUnionOf), Term::BNode(bnodeid)] => {
                    some! {
                        DisjointUnion(
                            Class(iri.clone()),
                            self.to_ce_seq(bnodeid)?
                        ).into()
                    }
                }
                [Term::Iri(p), Term::OWL(VOWL::InverseOf), Term::Iri(r)] => {
                    some! {
                        InverseObjectProperties (ObjectProperty(p.clone()),
                                                 ObjectProperty(r.clone())).into()
                    }
                }
                [pr, Term::RDF(VRDF::Type), Term::OWL(VOWL::TransitiveProperty)] => {
                    some! {
                        TransitiveObjectProperty(self.to_ope(pr)?).into()
                    }
                }
                [pr, Term::RDF(VRDF::Type), Term::OWL(VOWL::FunctionalProperty)] => {
                    some! {
                        match self.find_property_kind(pr)? {
                            PropertyExpression::ObjectPropertyExpression(ope) => {
                                FunctionalObjectProperty(ope).into()
                            },
                            PropertyExpression::DataProperty(dp) => {
                                FunctionalDataProperty(dp).into()
                            },
                            _ => todo!()
                        }
                    }
                }
                [pr, Term::RDF(VRDF::Type), Term::OWL(VOWL::AsymmetricProperty)] => {
                    some! {
                        match self.find_property_kind(pr)? {
                            PropertyExpression::ObjectPropertyExpression(ope) => {
                                AsymmetricObjectProperty(ope).into()
                            },

                            _ => todo!()
                        }
                    }
                }
                [pr, Term::RDF(VRDF::Type), Term::OWL(VOWL::SymmetricProperty)] => {
                    some! {
                        match self.find_property_kind(pr)? {
                            PropertyExpression::ObjectPropertyExpression(ope) => {
                                SymmetricObjectProperty(ope).into()
                            },

                            _ => todo!()
                        }
                    }
                }
                [pr, Term::RDF(VRDF::Type), Term::OWL(VOWL::ReflexiveProperty)] => {
                    some! {
                        match self.find_property_kind(pr)? {
                            PropertyExpression::ObjectPropertyExpression(ope) => {
                                ReflexiveObjectProperty(ope).into()
                            },

                            _ => todo!()
                        }
                    }
                }
                [pr, Term::RDF(VRDF::Type), Term::OWL(VOWL::IrreflexiveProperty)] => {
                    some! {
                        match self.find_property_kind(pr)? {
                            PropertyExpression::ObjectPropertyExpression(ope) => {
                                IrreflexiveObjectProperty(ope).into()
                            },

                            _ => todo!()
                        }
                    }
                }
                [pr, Term::RDF(VRDF::Type), Term::OWL(VOWL::InverseFunctionalProperty)] => {
                    some! {
                        match self.find_property_kind(pr)? {
                            PropertyExpression::ObjectPropertyExpression(ope) => {
                                InverseFunctionalObjectProperty(ope).into()
                            },

                            _ => todo!()
                        }
                    }
                }

                [Term::Iri(a), Term::OWL(VOWL::DisjointWith), Term::Iri(b)] => {
                    Some(
                        DisjointClasses(vec![
                            // The order is not important here, but
                            // this way around matches with the XML reader
                            Class(b.clone()).into(),
                            Class(a.clone()).into(),
                        ])
                        .into(),
                    )
                }
                [pr, Term::RDFS(VRDFS::SubPropertyOf), t] => {
                    some! {
                        match self.find_property_kind(t)? {
                            PropertyExpression::ObjectPropertyExpression(ope) =>
                                SubObjectPropertyOf {
                                    sub: self.to_sope(pr)?,
                                    sup: ope,
                                }.into(),
                            PropertyExpression::AnnotationProperty(ap) =>
                                SubAnnotationPropertyOf {
                                    sup: ap,
                                    sub: self.to_ap(pr)?
                                }.into(),
                            _ => todo!()
                        }
                    }
                }
                [Term::Iri(pr), Term::OWL(VOWL::PropertyChainAxiom), Term::BNode(id)] => {
                    some! {
                        SubObjectPropertyOf {
                            sub: SubObjectPropertyExpression::ObjectPropertyChain(
                                self.bnode_seq
                                    .remove(id)?
                                    .iter()
                                    .map(|t| self.to_ope(t).unwrap())
                                    .collect()
                            ),
                            sup: ObjectProperty(pr.clone()).into(),
                        }.into()
                    }
                }
                [pr, Term::RDFS(VRDFS::Domain), t] => {
                    some! {
                        match self.find_property_kind(pr)? {
                            PropertyExpression::ObjectPropertyExpression(ope) => ObjectPropertyDomain {
                                ope,
                                ce: self.to_ce(t)?,
                            }
                            .into(),
                            PropertyExpression::DataProperty(dp) => DataPropertyDomain {
                                dp,
                                ce: self.to_ce(t)?,
                            }
                            .into(),
                            PropertyExpression::AnnotationProperty(ap) => AnnotationPropertyDomain {
                                ap: ap,
                                iri: self.to_iri(t)?,
                            }
                            .into(),
                        }
                    }
                }
                [pr, Term::RDFS(VRDFS::Range), t] => some! {
                    match self.find_property_kind(pr)? {
                        PropertyExpression::ObjectPropertyExpression(ope) => ObjectPropertyRange {
                            ope,
                            ce: self.to_ce(t)?,
                        }
                        .into(),
                        PropertyExpression::DataProperty(dp) => DataPropertyRange {
                            dp,
                            dr: self.to_dr(t)?,
                        }
                        .into(),
                        PropertyExpression::AnnotationProperty(ap) => AnnotationPropertyRange {
                            ap: ap,
                            iri: self.to_iri(t)?,
                        }
                        .into(),
                    }
                },
                _ => None,
            };

            if let Some(axiom) = axiom {
                let ann = self.ann_map.remove(&triple).unwrap_or_else(|| BTreeSet::new());
                self.merge(AnnotatedAxiom {
                    axiom,
                    ann
                })
            } else {
                self.simple.push(triple)
            }
        }
    }

    fn simple_annotations(
        &mut self,
    ) {
        for triple in std::mem::take(&mut self.simple) {
            if let Some(iri) = match &triple {
                [Term::Iri(iri), Term::RDFS(rdfs), _] if rdfs.is_builtin() => Some(iri),
                [Term::Iri(iri), Term::OWL(VOWL::VersionInfo), _] => Some(iri),
                [Term::Iri(iri), Term::Iri(ap), _] if is_annotation_property(&self.o, &ap) => {
                    eprintln!("is annotation");
                    Some(iri)
                }
                _ => None,
            } {
                let ann = self.ann_map.remove(&triple).unwrap_or_else(|| BTreeSet::new());
                self.merge(AnnotatedAxiom {
                    axiom: AnnotationAssertion {
                        subject: iri.clone(),
                        ann: self.annotation(&triple),
                    }
                    .into(),
                    ann
                });
            } else {
                self.simple.push(triple);
            }
        }
    }

    fn read(mut self, triple: Vec<[SpTerm; 3]>) -> Result<Ontology, Error> {
        // move to our own Terms, with IRIs swapped

        let m = vocab_lookup();
        let triple: Vec<[Term; 3]> = triple
            .into_iter()
            .map(|t| {
                [
                    to_term(&t[0], &m, self.b),
                    to_term(&t[1], &m, self.b),
                    to_term(&t[2], &m, self.b),
                ]
            })
            .collect();

        Self::group_triples(triple, &mut self.simple, &mut self.bnode);

        // sort the triples, so that I can get a dependable order
        for (_, vec) in self.bnode.iter_mut() {
            vec.sort();
        }

        self.stitch_seqs();

        // Table 10
        self.axiom_annotations();

        self.resolve_imports();
        self.backward_compat();

        // for t in bnode.values() {
        //     match t.as_slice()[0] {
        //         [BNode(s), RDF(VRDF::First), ob] => {
        //             //let v = vec![];
        //             // So, we have captured first (value of which is ob)
        //             // Rest of the sequence could be either in
        //             // bnode_seq or in bnode -- confusing
        //             //bnode_seq.insert(s.clone(), self.seq())
        //         }
        //     }
        // }

        // Then handle SEQ this should give HashMap<BNodeID,
        // Vec<[SpTerm]> where the BNodeID is the first node of the
        // seq, and the SpTerms are the next in order. This will
        // require multiple passes through the triples (This is Table
        // 3 in the structural Specification)

        // At this point we should have everything we need to be able
        // to make all the entities that we need, already grouped into
        // a place we can access it.

        // Now we work through the tables in the RDF serialization

        // Table 4: headers. To do this fully requires imports also,
        // but we need to fudge this a little. We need to be to able
        // to read an ontology just for declarations. At the moment, I
        // don't know how to get to another set of triples for these
        // -- we will need some kind of factory.
        self.headers();

        // Can we pull out annotations at this point and handle them
        // as we do in reader2? Tranform them into a triple which we
        // handle normally, then bung the annotation on later?

        // Table 5: Backward compability -- skip this for now (maybe
        // for ever)

        // Table 6: Don't understand this

        // Table 7: Declarations (this should be simple, if we have a
        // generic solution for handling annotations, there is no
        // handling of bnodes).
        self.declarations();

        // Table 10
        self.simple_annotations();

        self.data_ranges();

        // Table 8:
        self.object_property_expressions();
        // Table 13: Parsing of Class Expressions
        self.class_expressions();

        // Table 16: Axioms without annotations
        self.axioms();


        // Regroup so that they print out nicer
        let mut simple_left = vec![];
        let mut bnode_left = HashMap::default();

        Self::group_triples(self.simple, &mut simple_left, &mut bnode_left);

        if simple_left.len() > 0 {
            dbg!("simple remaining", simple_left);
        }

        if bnode_left.len() > 0 {
            dbg!("bnode left", bnode_left);
        }

        if self.bnode_seq.len() > 0 {
            dbg!("sequences remaining", self.bnode_seq);
        }

        if self.ann_map.len() > 0 {
            dbg!("annotations remaining", self.ann_map);
        }

        if self.class_expression.len() > 0 {
            dbg!("class_expression remaining", self.class_expression);
        }

        if self.data_range.len() > 0 {
            dbg!("data range remaining", self.data_range);
        }

        if self.object_property_expression.len() > 0 {
            dbg!(
                "object property expression remaining",
                self.object_property_expression
            );
        }
        Ok(self.o)
    }
}

pub fn read_with_build<R: BufRead>(
    bufread: &mut R,
    build: &Build,
) -> Result<(Ontology, PrefixMapping), Error> {
    let parser = sophia::parser::xml::Config::default();
    let triple_iter = parser.parse_bufread(bufread);

    let triple_result: Result<Vec<_>, _> = triple_iter.collect();
    let triple_v: Vec<[SpTerm; 3]> = triple_result.map_err(SyncFailure::new)?;

    return OntologyParser::new(build)
        .read(triple_v)
        .map(|o| return (o, PrefixMapping::default()));
}

pub fn read<R: BufRead>(bufread: &mut R) -> Result<(Ontology, PrefixMapping), Error> {
    let b = Build::new();
    read_with_build(bufread, &b)
}

#[cfg(test)]
mod test {
    use super::*;

    use std::io::Write;
    use std::path::PathBuf;

    use pretty_assertions::assert_eq;

    fn init_log() {
        let _ = env_logger::builder()
            .format(|buf, record| writeln!(buf, "{}", record.args()))
            .is_test(true)
            .try_init();
    }

    fn read_ok<R: BufRead>(bufread: &mut R) -> (Ontology, PrefixMapping) {
        init_log();

        let r = read(bufread);

        assert!(r.is_ok(), "Expected ontology, get failure: {:?}", r.err());
        r.unwrap()
    }

    fn compare(test: &str) {
        compare_two(test, test);
    }

    fn compare_two(testrdf: &str, testowl: &str) {
        let dir_path_buf = PathBuf::from(file!());
        let dir = dir_path_buf.parent().unwrap().to_string_lossy();

        compare_str(
            &slurp::read_all_to_string(format!("{}/../../ont/owl-rdf/{}.owl", dir, testrdf))
                .unwrap(),
            &slurp::read_all_to_string(format!("{}/../../ont/owl-xml/{}.owx", dir, testowl))
                .unwrap(),
        );
    }

    fn compare_str(rdfread: &str, xmlread: &str) {
        let (rdfont, _rdfmapping) = read_ok(&mut rdfread.as_bytes());
        let (xmlont, _xmlmapping) = crate::io::reader::test::read_ok(&mut xmlread.as_bytes());

        //dbg!(&rdfont); if true {panic!()};

        assert_eq!(rdfont, xmlont);

        //let rdfmapping: &HashMap<&String, &String> = &rdfmapping.mappings().collect();
        //let xmlmapping: &HashMap<&String, &String> = &xmlmapping.mappings().collect();

        //assert_eq!(rdfmapping, xmlmapping);
    }

    #[test]
    fn class() {
        compare("class");
    }

    #[test]
    fn declaration_with_annotation() {
        compare("declaration-with-annotation");
    }

    #[test]
    fn declaration_with_two_annotation() {
        compare("declaration-with-two-annotation");
    }

    #[test]
    fn class_with_two_annotations() {
        compare("class_with_two_annotations");
    }

    #[test]
    fn ont() {
        compare("ont");
    }

    #[test]
    fn one_subclass() {
        compare("one-subclass");
    }

    #[test]
    fn subclass_with_annotation() {
        compare("annotation-on-subclass");
    }

    #[test]
    fn oproperty() {
        compare("oproperty");
    }

    #[test]
    fn some() {
        compare("some");
    }

    #[test]
    fn some_not() {
        compare("some-not");
    }

    #[test]
    fn one_some_reversed() {
        compare_two("manual/one-some-reversed-triples", "some");
    }

    #[test]
    fn one_some_property_filler_reversed() {
        compare_two("manual/one-some-property-filler-reversed", "some");
    }

    #[test]
    fn only() {
        compare("only");
    }

    #[test]
    fn and() {
        compare("and");
    }

    #[test]
    fn or() {
        compare("or");
    }

    #[test]
    fn not() {
        compare("not");
    }

    #[test]
    fn annotation_property() {
        compare("annotation-property");
    }

    #[test]
    fn annotation() {
        compare("annotation");
    }

    #[test]
    fn annotation_domain() {
        compare("annotation-domain");
    }

    #[test]
    fn annotation_range() {
        compare("annotation-range");
    }

    #[test]
    fn label() {
        compare("label");
    }

    #[test]
    fn one_comment() {
        // This is currently failing because the XML parser gives the
        // comment a language and a datatype ("PlainLiteral") while
        // the RDF one gives it just the language, as literals can't
        // be both. Which is correct?
        compare("one-comment");
    }

    // #[test]
    // fn one_ontology_annotation() {
    //     compare("one-ontology-annotation");
    // }

    #[test]
    fn one_equivalent_class() {
        compare("one-equivalent");
    }

    #[test]
    fn one_disjoint_class() {
        compare("one-disjoint");
    }

    #[test]
    fn disjoint_union() {
        compare("disjoint-union");
    }

    #[test]
    fn sub_oproperty() {
        compare("suboproperty");
    }

    #[test]
    fn sub_oproperty_inverse() {
        compare("suboproperty-inverse");
    }

    #[test]
    fn one_inverse() {
        compare("inverse-properties");
    }

    #[test]
    fn one_transitive() {
        compare("transitive-properties");
    }

    #[test]
    fn inverse_transitive() {
        compare("inverse-transitive")
    }

    #[test]
    fn one_annotated_transitive() {
        compare("annotation-on-transitive");
    }

    #[test]
    fn subproperty_chain() {
        compare("subproperty-chain");
    }

    // #[test]
    // fn one_subproperty_chain_with_inverse() {
    //     compare("subproperty-chain-with-inverse");
    // }

    #[test]
    fn annotation_on_annotation() {
        compare("annotation-with-annotation");
    }

    #[test]
    fn non_built_in_annotation_on_annotation() {
        compare("annotation-with-non-builtin-annotation");
    }

    #[test]
    fn sub_annotation() {
        compare("sub-annotation");
    }

    #[test]
    fn data_property() {
        compare("data-property");
    }

    // #[test]
    // fn literal_escaped() {
    //     compare("literal-escaped");
    // }

    #[test]
    fn named_individual() {
        compare("named-individual");
    }

    // #[test]
    // fn import() {
    //     compare("import");
    // }

    #[test]
    fn datatype() {
        compare("datatype");
    }

    #[test]
    fn object_has_value() {
        compare("object-has-value");
    }

    #[test]
    fn object_one_of() {
        compare("object-one-of");
    }

    #[test]
    fn inverse() {
        compare("some-inverse");
    }

    #[test]
    fn object_unqualified_cardinality() {
        compare("object-unqualified-max-cardinality");
    }

    #[test]
    fn object_min_cardinality() {
        compare("object-min-cardinality");
    }

    #[test]
    fn object_max_cardinality() {
        compare("object-max-cardinality");
    }

    #[test]
    fn object_exact_cardinality() {
        compare("object-exact-cardinality");
    }

    #[test]
    fn datatype_alias() {
        compare("datatype-alias");
    }

    #[test]
    fn datatype_intersection() {
        compare("datatype-intersection");
    }

    #[test]
    fn datatype_union() {
        compare("datatype-union");
    }

    #[test]
    fn datatype_complement() {
        compare("datatype-complement");
    }

    // #[test]
    // fn datatype_oneof() {
    //     compare("datatype-oneof");
    // }

    #[test]
    fn datatype_some() {
        compare("data-some");
    }

    // #[test]
    // fn facet_restriction() {
    //     compare("facet-restriction");
    // }

    #[test]
    fn data_only() {
        compare("data-only");
    }

    #[test]
    fn data_exact_cardinality() {
        compare("data-exact-cardinality");
    }

    // #[test]
    // fn data_has_value() {
    //     compare("data-has-value");
    // }

    // #[test]
    // fn data_max_cardinality() {
    //     compare("data-max-cardinality");
    // }

    // #[test]
    // fn data_min_cardinality() {
    //     compare("data-min-cardinality");
    // }

    // #[test]
    // fn class_assertion() {
    //     compare("class-assertion");
    // }

    // #[test]
    // fn data_property_assertion() {
    //     compare("data-property-assertion");
    // }

    // #[test]
    // fn same_individual() {
    //     compare("same-individual");
    // }

    // #[test]
    // fn different_individuals() {
    //     compare("different-individual");
    // }

    // #[test]
    // fn negative_data_property_assertion() {
    //     compare("negative-data-property-assertion");
    // }

    // #[test]
    // fn negative_object_property_assertion() {
    //     compare("negative-object-property-assertion");
    // }

    // #[test]
    // fn object_property_assertion() {
    //     compare("object-property-assertion");
    // }

    // #[test]
    // fn data_has_key() {
    //     compare("data-has-key");
    // }

    // #[test]
    // fn data_property_disjoint() {
    //     compare("data-property-disjoint");
    // }

    #[test]
    fn data_property_domain() {
        compare("data-property-domain");
    }

    // #[test]
    // fn data_property_equivalent() {
    //     compare("data-property-equivalent");
    // }

    #[test]
    fn data_property_functional() {
        compare("data-property-functional");
    }

    #[test]
    fn data_property_range() {
        compare("data-property-range");
    }

    // #[test]
    // fn data_property_sub() {
    //     compare("data-property-sub");
    // }

    // #[test]
    // fn disjoint_object_properties() {
    //     compare("disjoint-object-properties");
    // }

    // #[test]
    // fn equivalent_object_properties() {
    //     compare("equivalent_object_properties");
    // }

    // #[test]
    // fn object_has_key() {
    //     compare("object-has-key");
    // }

    #[test]
    fn object_property_asymmetric() {
        compare("object-property-asymmetric");
    }

    #[test]
    fn object_property_domain() {
        compare("object-property-domain");
    }

    #[test]
    fn object_property_functional() {
        compare("object-property-functional");
    }

    #[test]
    fn object_property_inverse_functional() {
        compare("object-property-inverse-functional");
    }

    #[test]
    fn object_property_irreflexive() {
        compare("object-property-irreflexive");
    }

    #[test]
    fn object_property_range() {
        compare("object-property-range");
    }

    #[test]
    fn object_property_reflexive() {
        compare("object-property-reflexive");
    }

    #[test]
    fn object_property_symmetric() {
        compare("object-property-symmetric");
    }

    // #[test]
    // fn family() {
    //     compare("family");
    // }
}
