//! Streaming projection of a manifest. Canonical products put `id` first, so
//! unselected bodies are consumed with IgnoredAny without allocating their arrays.
use super::*;
use serde::de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use std::fmt;

fn valid(id: &str) -> bool {
    id.starts_with('p') && id.len() > 1 && id[1..].bytes().all(|b| b.is_ascii_digit())
}
struct ProductSeed<'a> {
    wanted: &'a HashSet<&'a str>,
    known: &'a mut HashSet<String>,
}
impl<'de> DeserializeSeed<'de> for ProductSeed<'_> {
    type Value = Option<Product>;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_map(self)
    }
}
impl<'de> Visitor<'de> for ProductSeed<'_> {
    type Value = Option<Product>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a manifest product")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut body = serde_json::Map::new();
        let mut id = None;
        let mut keep = None;
        let mut fields = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !fields.insert(key.clone()) {
                return Err(de::Error::custom(format!("duplicate product field {key}")));
            }
            if key == "id" {
                let value: String = map.next_value()?;
                if !valid(&value) || !self.known.insert(value.clone()) {
                    return Err(de::Error::custom(format!(
                        "invalid or duplicate manifest product id {value}"
                    )));
                }
                keep = Some(self.wanted.contains(value.as_str()));
                if keep == Some(false) {
                    body.clear();
                }
                id = Some(value);
            } else if keep == Some(false) {
                map.next_value::<IgnoredAny>()?;
            } else {
                // Field order is not a JSON contract. A noncanonical pre-id prefix
                // is buffered for this one product, never for the entire file.
                body.insert(key, map.next_value()?);
            }
        }
        let id = id.ok_or_else(|| de::Error::missing_field("id"))?;
        if keep == Some(false) {
            return Ok(None);
        }
        body.insert("id".into(), id.into());
        serde_json::from_value(body.into())
            .map(Some)
            .map_err(de::Error::custom)
    }
}
struct Products<'a> {
    wanted: &'a HashSet<&'a str>,
    known: &'a mut HashSet<String>,
}
impl<'de> DeserializeSeed<'de> for Products<'_> {
    type Value = Vec<Product>;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_seq(self)
    }
}
impl<'de> Visitor<'de> for Products<'_> {
    type Value = Vec<Product>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("manifest products")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut out = Vec::new();
        while let Some(p) = seq.next_element_seed(ProductSeed {
            wanted: self.wanted,
            known: self.known,
        })? {
            if let Some(p) = p {
                out.push(p);
            }
        }
        Ok(out)
    }
}
struct Instances<'a, F> {
    callback: &'a mut F,
    references: &'a mut HashSet<String>,
}
impl<'de, F: FnMut(Instance) -> Result<(), StepError>> DeserializeSeed<'de> for Instances<'_, F> {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_seq(self)
    }
}
impl<'de, F: FnMut(Instance) -> Result<(), StepError>> Visitor<'de> for Instances<'_, F> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("manifest instances")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        while let Some(i) = seq.next_element::<Instance>()? {
            if !valid(&i.product) {
                return Err(de::Error::custom(format!(
                    "invalid instance product id {}",
                    i.product
                )));
            }
            self.references.insert(i.product.clone());
            (self.callback)(i).map_err(de::Error::custom)?;
        }
        Ok(())
    }
}
struct Selected<'a, F> {
    ids: &'a [String],
    callback: &'a mut F,
}
impl<'de, F: FnMut(Instance) -> Result<(), StepError>> DeserializeSeed<'de> for Selected<'_, F> {
    type Value = Manifest;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Manifest, D::Error> {
        d.deserialize_map(self)
    }
}
impl<'de, F: FnMut(Instance) -> Result<(), StepError>> Visitor<'de> for Selected<'_, F> {
    type Value = Manifest;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an assembly manifest")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Manifest, A::Error> {
        let wanted: HashSet<_> = self.ids.iter().map(String::as_str).collect();
        let mut known = HashSet::new();
        let mut references = HashSet::new();
        let mut fields = HashSet::new();
        let (mut format, mut source, mut reader, mut common, mut products, mut refusals) =
            (None, None, None, None, None, None);
        while let Some(key) = map.next_key::<String>()? {
            if !fields.insert(key.clone()) {
                return Err(de::Error::custom(format!("duplicate manifest field {key}")));
            }
            match key.as_str() {
                "format" => format = Some(map.next_value()?),
                "source" => source = Some(map.next_value()?),
                "reader" => reader = Some(map.next_value()?),
                "common" => common = Some(map.next_value()?),
                "products" => {
                    products = Some(map.next_value_seed(Products {
                        wanted: &wanted,
                        known: &mut known,
                    })?)
                }
                "refusals" => refusals = Some(map.next_value::<Vec<Refusal>>()?),
                "instances" => map.next_value_seed(Instances {
                    callback: self.callback,
                    references: &mut references,
                })?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        for name in ["assemblies", "instances", "unowned"] {
            if !fields.contains(name) {
                return Err(de::Error::missing_field(name));
            }
        }
        let unknown: Vec<_> = self
            .ids
            .iter()
            .filter(|id| !known.contains(id.as_str()))
            .map(String::as_str)
            .collect();
        if !unknown.is_empty() {
            return Err(de::Error::custom(format!(
                "unknown manifest product ids: {}",
                unknown.join(", ")
            )));
        }
        if let Some(id) = references.iter().find(|id| !known.contains(*id)) {
            return Err(de::Error::custom(format!(
                "instance references unknown product {id}"
            )));
        }
        let refusals = refusals.ok_or_else(|| de::Error::missing_field("refusals"))?;
        for r in &refusals {
            for id in &r.products {
                if !valid(id) || !known.contains(id) {
                    return Err(de::Error::custom(format!(
                        "refusal references unknown product {id}"
                    )));
                }
            }
        }
        Manifest::try_from(WireManifest {
            format: format.ok_or_else(|| de::Error::missing_field("format"))?,
            source: source.ok_or_else(|| de::Error::missing_field("source"))?,
            reader: reader.ok_or_else(|| de::Error::missing_field("reader"))?,
            common: common.ok_or_else(|| de::Error::missing_field("common"))?,
            products: products.ok_or_else(|| de::Error::missing_field("products"))?,
            refusals,
            assemblies: Vec::new(),
            instances: Vec::new(),
            unowned: Unowned::default(),
        })
        .map_err(de::Error::custom)
    }
}
impl Manifest {
    /// Load selected products without retaining other products, assemblies, unowned
    /// ranges or instances. Every product id is checked for safety and uniqueness;
    /// every instance is delivered to `on_instance` in file order. Callback effects
    /// must be staged until this function succeeds (a later field can be invalid).
    /// Use a BufReader for files. Canonical id-first products skip all unselected
    /// bodies; a reordered pre-id prefix buffers at most one product.
    pub fn read_selected<R: Read>(
        reader: R,
        ids: &[String],
        mut on_instance: impl FnMut(Instance) -> Result<(), StepError>,
    ) -> Result<Self, StepError> {
        let mut d = serde_json::Deserializer::from_reader(reader);
        let m = Selected {
            ids,
            callback: &mut on_instance,
        }
        .deserialize(&mut d)
        .map_err(failure)?;
        d.end().map_err(failure)?;
        Ok(m)
    }
}
