use agenterm_tinyvm::{
    GuestResourceHandle, HostResourceTable, MAX_RESOURCE_GENERATION, MAX_RESOURCE_SLOTS,
    ResourceHandleDomain, ResourceTableError,
};
use std::cell::Cell;
use std::rc::Rc;

fn domain(raw: u8) -> ResourceHandleDomain {
    ResourceHandleDomain::new(raw).expect("non-zero test domain")
}

#[test]
fn guest_handle_round_trips_as_an_i32_token() {
    let mut table = HostResourceTable::new(domain(7), 2).expect("create table");
    let handle = table.insert(String::from("texture")).expect("insert");

    assert_ne!(handle.raw(), 0);
    assert_eq!(GuestResourceHandle::from_i32(handle.as_i32()), Some(handle));
    assert_eq!(GuestResourceHandle::from_raw(0), None);
    assert_eq!(GuestResourceHandle::from_raw(1), None);
    assert_eq!(GuestResourceHandle::from_raw(1 << 12), None);
    assert_eq!(GuestResourceHandle::from_raw(1 << 24), None);
    assert_eq!(handle.domain(), domain(7));
    assert_eq!(table.get(handle).map(String::as_str), Ok("texture"));
}

#[test]
fn removed_handle_never_names_a_reused_slot() {
    let mut table = HostResourceTable::new(domain(1), 1).expect("create table");
    let first = table.insert(10).expect("first insert");
    assert_eq!(table.remove(first), Ok(10));

    let second = table.insert(20).expect("reuse slot");
    assert_ne!(first, second);
    assert_eq!(table.get(first), Err(ResourceTableError::StaleHandle));
    assert_eq!(table.get(second), Ok(&20));
    *table.get_mut(second).expect("mutate live resource") = 21;
    assert_eq!(table.remove(second), Ok(21));
    assert_eq!(table.remove(second), Err(ResourceTableError::StaleHandle));
}

#[test]
fn capacity_is_bounded_and_failed_insert_drops_its_value() {
    struct Counted(Rc<Cell<u32>>);
    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    let drops = Rc::new(Cell::new(0));
    let mut table = HostResourceTable::new(domain(1), 1).expect("create table");
    let live = table.insert(Counted(drops.clone())).expect("insert live");
    assert!(!table.has_capacity());
    assert!(matches!(
        table.insert(Counted(drops.clone())),
        Err(ResourceTableError::Full)
    ));
    assert_eq!(drops.get(), 1);
    assert_eq!(table.len(), 1);

    drop(table.remove(live).expect("remove live"));
    assert_eq!(drops.get(), 2);
    assert!(table.is_empty());
    assert!(table.has_capacity());
}

#[test]
fn clear_drops_resources_and_invalidates_all_handles() {
    let mut table = HostResourceTable::new(domain(1), 3).expect("create table");
    let a = table.insert(1).expect("insert a");
    let b = table.insert(2).expect("insert b");
    table.clear();

    assert!(table.is_empty());
    assert_eq!(table.get(a), Err(ResourceTableError::StaleHandle));
    assert_eq!(table.get(b), Err(ResourceTableError::StaleHandle));
    let next = table.insert(3).expect("insert after clear");
    assert_ne!(next, a);
    assert_eq!(table.get(next), Ok(&3));
}

#[test]
fn exhausted_generation_retires_instead_of_aliasing_an_old_handle() {
    let mut table = HostResourceTable::new(domain(1), 1).expect("create table");
    let oldest = table.insert(()).expect("first generation");
    let mut current = oldest;

    for _ in 1..MAX_RESOURCE_GENERATION {
        table.remove(current).expect("remove generation");
        current = table.insert(()).expect("next generation");
    }
    table.remove(current).expect("retire final generation");

    assert!(!table.has_capacity());
    assert_eq!(table.insert(()), Err(ResourceTableError::Full));
    assert_eq!(table.get(oldest), Err(ResourceTableError::StaleHandle));
}

#[test]
fn module_domains_reject_cross_table_handle_collisions() {
    let mut texture = HostResourceTable::new(domain(1), 1).expect("texture table");
    let mut audio = HostResourceTable::new(domain(2), 1).expect("audio table");
    let texture_handle = texture.insert("texture").expect("insert texture");
    let audio_handle = audio.insert("audio").expect("insert audio");

    assert_ne!(texture_handle, audio_handle);
    assert_eq!(
        texture.get(audio_handle),
        Err(ResourceTableError::StaleHandle)
    );
    assert_eq!(
        audio.get(texture_handle),
        Err(ResourceTableError::StaleHandle)
    );
    assert_eq!(texture.get(texture_handle), Ok(&"texture"));
    assert_eq!(audio.get(audio_handle), Ok(&"audio"));

    assert!(matches!(
        HostResourceTable::<()>::new(domain(3), MAX_RESOURCE_SLOTS + 1),
        Err(ResourceTableError::InvalidLimit)
    ));
}
