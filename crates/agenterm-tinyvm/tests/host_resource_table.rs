use agenterm_tinyvm::{GuestResourceHandle, HostResourceTable, ResourceTableError};
use std::cell::Cell;
use std::rc::Rc;

#[test]
fn guest_handle_round_trips_as_an_i32_token() {
    let mut table = HostResourceTable::new(2);
    let handle = table.insert(String::from("texture")).expect("insert");

    assert_ne!(handle.raw(), 0);
    assert_eq!(GuestResourceHandle::from_i32(handle.as_i32()), Some(handle));
    assert_eq!(GuestResourceHandle::from_raw(0), None);
    assert_eq!(GuestResourceHandle::from_raw(1), None);
    assert_eq!(GuestResourceHandle::from_raw(1 << 16), None);
    assert_eq!(table.get(handle).map(String::as_str), Ok("texture"));
}

#[test]
fn removed_handle_never_names_a_reused_slot() {
    let mut table = HostResourceTable::new(1);
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
    let mut table = HostResourceTable::new(1);
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
    let mut table = HostResourceTable::new(3);
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
    let mut table = HostResourceTable::new(1);
    let oldest = table.insert(()).expect("first generation");
    let mut current = oldest;

    for _ in 1..u16::MAX {
        table.remove(current).expect("remove generation");
        current = table.insert(()).expect("next generation");
    }
    table.remove(current).expect("retire final generation");

    assert!(!table.has_capacity());
    assert_eq!(table.insert(()), Err(ResourceTableError::Full));
    assert_eq!(table.get(oldest), Err(ResourceTableError::StaleHandle));
}
