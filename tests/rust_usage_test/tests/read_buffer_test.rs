//! Tests for custom `flatbuffers::ReadBuffer` implementations: every read the
//! generated code performs must be observable through `ReadBuffer::bytes`.

extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use flatbuffers::{ReadBuffer, TaggedUnion, TaggedUnionWIPOffset, WIPOffset};

#[allow(dead_code, unused_imports, clippy::all)]
#[path = "../../monster_test/mod.rs"]
mod monster_test_generated;
use monster_test_generated::my_game::example::*;

#[allow(dead_code, unused_imports, clippy::all)]
#[path = "../../more_defaults/mod.rs"]
mod more_defaults_generated;

const PAGE: usize = 64;

/// Records how often each byte was handed out and which "pages" were hit.
#[derive(Default)]
struct Recorder {
    touched: Mutex<Vec<u32>>,
    pages: Mutex<Vec<usize>>,
    calls: AtomicUsize,
}

struct TracedBuf<'m> {
    data: &'m [u8],
    rec: Option<&'m Recorder>,
}

static EMPTY: TracedBuf<'static> = TracedBuf { data: &[0; 4], rec: None };

unsafe impl ReadBuffer for TracedBuf<'_> {
    unsafe fn bytes(&self, offset: usize, len: usize) -> &[u8] {
        if let Some(rec) = self.rec {
            rec.calls.fetch_add(1, Ordering::Relaxed);
            if len != 0 {
                let mut t = rec.touched.lock().unwrap();
                for b in &mut t[offset..offset + len] {
                    *b += 1;
                }
                rec.pages
                    .lock()
                    .unwrap()
                    .extend(offset / PAGE..=(offset + len - 1) / PAGE);
            }
        }
        &self.data[offset..offset + len]
    }

    fn len(&self) -> usize {
        self.data.len()
    }

    fn empty_vector<'a>() -> Option<&'a Self>
    where
        Self: 'a,
    {
        Some(&EMPTY)
    }
}

impl<'m> TracedBuf<'m> {
    fn new(data: &'m [u8], rec: &'m Recorder) -> Self {
        *rec.touched.lock().unwrap() = vec![0; data.len()];
        TracedBuf { data, rec: Some(rec) }
    }

    fn reset(&self) {
        let rec = self.rec.unwrap();
        rec.touched.lock().unwrap().iter_mut().for_each(|b| *b = 0);
        rec.pages.lock().unwrap().clear();
        rec.calls.store(0, Ordering::Relaxed);
    }

    fn calls(&self) -> usize {
        self.rec.unwrap().calls.load(Ordering::Relaxed)
    }

    /// Asserts every byte of `s` (a sub-slice of `data`) was handed out by `bytes`.
    fn assert_covered(&self, s: &[u8], what: &str) {
        let start = s.as_ptr() as usize - self.data.as_ptr() as usize;
        assert!(start + s.len() <= self.data.len(), "{} is not inside the buffer", what);
        let t = self.rec.unwrap().touched.lock().unwrap();
        for (i, b) in t[start..start + s.len()].iter().enumerate() {
            assert!(*b > 0, "{}: byte {} at offset {} was read without ReadBuffer::bytes", what, i, start + i);
        }
    }
}

fn build_monster() -> Vec<u8> {
    let mut b = flatbuffers::FlatBufferBuilder::new();

    let nested = {
        let mut nb = flatbuffers::FlatBufferBuilder::new();
        let name = nb.create_string("Nested");
        let m = Monster::create(&mut nb, &MonsterArgs { name: Some(name), hp: 7, ..Default::default() });
        finish_monster_buffer(&mut nb, m);
        nb.finished_data().to_vec()
    };
    let nested = b.create_vector(&nested[..]);

    let fred = b.create_string("Fred");
    let fred = Monster::create(&mut b, &MonsterArgs { name: Some(fred), ..Default::default() });

    let tables: Vec<_> = ["a", "b", "c"]
        .iter()
        .map(|n| {
            let n = b.create_string(n);
            Monster::create(&mut b, &MonsterArgs { name: Some(n), ..Default::default() })
        })
        .collect();
    let tables = b.create_vector(&tables);

    let s0 = b.create_string("test1");
    let s1 = b.create_string("test2");
    let strings = b.create_vector(&[s0, s1]);
    let inventory = b.create_vector(&[0u8, 1, 2, 3, 4]);
    let enums = b.create_vector(&[Color::Red, Color::Blue]);
    let name = b.create_string("MyMonster");
    let pos = Vec3::new(1.0, 2.0, 3.0, 3.0, Color::Green, &Test::new(5, 6));

    let m = Monster::create(
        &mut b,
        &MonsterArgs {
            hp: 80,
            name: Some(name),
            pos: Some(&pos),
            test_type: Any::Monster,
            test: Some(fred.as_union_value()),
            inventory: Some(inventory),
            testarrayofstring: Some(strings),
            testarrayoftables: Some(tables),
            testnestedflatbuffer: Some(nested),
            vector_of_enums: Some(enums),
            ..Default::default()
        },
    );
    finish_monster_buffer(&mut b, m);
    b.finished_data().to_vec()
}

#[test]
fn read_buffer_counting_observes_all_reads() {
    let data = build_monster();
    // Verification runs on the plain slice and is not traced.
    root_as_monster(&data).unwrap();

    let rec = Recorder::default();
    let traced = TracedBuf::new(&data, &rec);
    let m = unsafe { root_as_monster_unchecked_from(&traced) };

    traced.reset();
    assert_eq!(m.hp(), 80);
    assert!(traced.calls() >= 4, "scalar field read: soffset, vtable len, voffset, value");

    traced.reset();
    let name = m.name();
    assert_eq!(name, "MyMonster");
    traced.assert_covered(name.as_bytes(), "name");

    traced.reset();
    let pos = m.pos().unwrap();
    assert_eq!(pos.x(), 1.0);
    let pos_bytes = unsafe {
        core::slice::from_raw_parts(pos as *const Vec3 as *const u8, core::mem::size_of::<Vec3>())
    };
    traced.assert_covered(pos_bytes, "pos struct");

    traced.reset();
    let inv = m.inventory().unwrap();
    assert_eq!(inv.iter().collect::<Vec<u8>>(), [0, 1, 2, 3, 4]);
    traced.assert_covered(inv.bytes(), "inventory");

    traced.reset();
    let fred = m.test_as_monster().unwrap();
    traced.assert_covered(fred.name().as_bytes(), "union member name");

    traced.reset();
    let strings: Vec<&str> = m.testarrayofstring().unwrap().iter().collect();
    assert_eq!(strings, ["test1", "test2"]);
    for s in &strings {
        traced.assert_covered(s.as_bytes(), "vector of strings");
    }

    traced.reset();
    let enums: Vec<Color> = m.vector_of_enums().unwrap().iter().collect();
    assert_eq!(enums, [Color::Red, Color::Blue]);
    traced.assert_covered(m.vector_of_enums().unwrap().bytes(), "vector of enums");

    traced.reset();
    let tables = m.testarrayoftables().unwrap();
    let found = tables.lookup_by_key("b", |t, k| t.key_compare_with_value(k)).unwrap();
    traced.assert_covered(found.name().as_bytes(), "lookup_by_key");
    assert!(tables.get(0).key_compare_less_than(&tables.get(1)));

    traced.reset();
    let nested = m.testnestedflatbuffer_nested_flatbuffer().unwrap();
    assert_eq!(nested.hp(), 7);
    assert_eq!(nested.name(), "Nested");
    traced.assert_covered(nested.name().as_bytes(), "nested flatbuffer");

    // Every recorded page lies inside the buffer.
    let last_page = (data.len() - 1) / PAGE;
    assert!(rec.pages.lock().unwrap().iter().all(|p| *p <= last_page));
}

#[test]
fn read_buffer_zero_length_reads() {
    let mut b = flatbuffers::FlatBufferBuilder::new();
    let name = b.create_string("");
    let inv = b.create_vector::<u8>(&[]);
    let m = Monster::create(&mut b, &MonsterArgs { name: Some(name), inventory: Some(inv), ..Default::default() });
    finish_monster_buffer(&mut b, m);
    let data = b.finished_data();

    let rec = Recorder::default();
    let traced = TracedBuf::new(data, &rec);
    let m = unsafe { root_as_monster_unchecked_from(&traced) };
    assert_eq!(m.name(), "");
    assert!(m.inventory().unwrap().is_empty());
    assert!(m.inventory().unwrap().bytes().is_empty());
}

#[test]
fn read_buffer_default_vector_value() {
    use more_defaults_generated::MoreDefaults;
    let data = [0u8; 4];
    flatbuffers::root::<MoreDefaults>(&data).unwrap();
    let rec = Recorder::default();
    let traced = TracedBuf::new(&data, &rec);
    let m = unsafe { flatbuffers::root_unchecked_from::<MoreDefaults<'_, TracedBuf>, _>(&traced) };
    assert_eq!(m.ints().len(), 0);
    assert_eq!(m.bools().iter().count(), 0);
}

#[test]
fn plain_slice_and_traced_buffer_agree() {
    let data = build_monster();
    let rec = Recorder::default();
    let traced = TracedBuf::new(&data, &rec);
    let a = root_as_monster(&data).unwrap();
    let b = unsafe { root_as_monster_unchecked_from(&traced) };
    // `MonsterT` has NaN defaults, so compare the Debug output.
    assert_eq!(format!("{:?}", a.unpack()), format!("{:?}", b.unpack()));
}

struct StrUnion;
impl TaggedUnion for StrUnion {
    type Tag = u8;
}

fn str_item(o: WIPOffset<&str>, tag: u8) -> TaggedUnionWIPOffset<StrUnion> {
    TaggedUnionWIPOffset::new(tag, WIPOffset::new(o.value()))
}

#[test]
fn union_vector_builder_reuse_and_layout() {
    let mut b = flatbuffers::FlatBufferBuilder::new();
    let x = b.create_string("x");
    let y = b.create_string("y");

    let mut vectors = Vec::new();
    for _ in 0..2 {
        b.start_union_vector::<StrUnion>(2);
        // Pushed in reverse order, like `start_vector`.
        b.push_union_vector_item(str_item(y, 2));
        b.push_union_vector_item(str_item(x, 1));
        vectors.push(b.end_union_vector::<StrUnion>(2));
    }

    let t = b.start_table();
    b.push_slot_always(4, vectors[1].tags());
    b.push_slot_always(6, vectors[1].values_offset());
    let t = b.end_table(t);
    b.finish_minimal(t);
    let data = b.finished_data();

    let table = unsafe { flatbuffers::root_unchecked::<flatbuffers::Table>(data) };
    let tags = unsafe {
        table.get::<flatbuffers::ForwardsUOffset<flatbuffers::Vector<u8>>>(4, None).unwrap()
    };
    let vals = unsafe {
        table
            .get::<flatbuffers::ForwardsUOffset<flatbuffers::Vector<flatbuffers::ForwardsUOffset<&str>>>>(6, None)
            .unwrap()
    };
    assert_eq!(tags.iter().collect::<Vec<_>>(), [1, 2]);
    assert_eq!(vals.iter().collect::<Vec<_>>(), ["x", "y"]);
}

#[test]
#[should_panic(expected = "more items pushed than announced")]
fn union_vector_builder_too_many_items_panics() {
    let mut b = flatbuffers::FlatBufferBuilder::new();
    let x = b.create_string("x");
    b.start_union_vector::<StrUnion>(1);
    b.push_union_vector_item(str_item(x, 1));
    b.push_union_vector_item(str_item(x, 1));
}

#[test]
#[should_panic(expected = "not enough union values")]
fn union_vector_builder_too_few_items_panics() {
    let mut b = flatbuffers::FlatBufferBuilder::new();
    let x = b.create_string("x");
    b.start_union_vector::<StrUnion>(2);
    b.push_union_vector_item(str_item(x, 1));
    b.end_union_vector::<StrUnion>(2);
}
