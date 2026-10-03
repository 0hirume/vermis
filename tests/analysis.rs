use std::{collections::HashSet, sync::Arc};

use vermis::{AnalysisError, Memo, Span, parse};

#[test]
fn reuse_tracks_identity_across_shifted_snapshots_and_environments() {
    let original = parse(b"local first = 1\nlocal second = 2\nreturn second\n");

    let statement = original
        .root()
        .children()
        .next()
        .unwrap()
        .children()
        .nth(1)
        .unwrap();

    let identity = statement.identity();

    let updated = original
        .update(Span { start: 14, end: 15 }, b"123456")
        .unwrap();

    let shifted = updated
        .root()
        .children()
        .next()
        .unwrap()
        .children()
        .nth(1)
        .unwrap();

    assert_ne!(statement.span(), shifted.span());
    assert_eq!(identity, shifted.identity());

    assert_eq!(
        HashSet::from([identity.clone(), shifted.identity()]).len(),
        1
    );

    let separate = parse(original.source());

    let separate_statement = separate
        .root()
        .children()
        .next()
        .unwrap()
        .children()
        .nth(1)
        .unwrap();

    assert_eq!(statement.text(), separate_statement.text());
    assert_ne!(identity, separate_statement.identity());

    let mut memo = Memo::new(4);

    let value = memo
        .query(identity.clone(), 1, |_| Ok(statement.text().to_vec()))
        .unwrap();

    let reused = memo
        .query(shifted.identity(), 1, |_| {
            panic!("unchanged syntax must reuse its result")
        })
        .unwrap();

    assert!(Arc::ptr_eq(&value, &reused));

    let changed = memo
        .query(shifted.identity(), 2, |_| {
            Ok(b"different environment".to_vec())
        })
        .unwrap();

    assert!(!Arc::ptr_eq(&value, &changed));
    memo.invalidate(&identity, &1);
    assert_eq!(memo.len(), 1);

    let retained = memo
        .query(shifted.identity(), 2, |_| {
            panic!("other environment remains cached")
        })
        .unwrap();

    assert!(Arc::ptr_eq(&changed, &retained));

    assert_eq!(
        original.source(),
        b"local first = 1\nlocal second = 2\nreturn second\n"
    );
}

#[test]
fn cached_dependencies_invalidate_transitively() {
    let identity = parse(b"return 1").root().identity();
    let mut memo = Memo::new(8);
    let independent = memo.query(identity.clone(), 9, |_| Ok(90)).unwrap();
    memo.query(identity.clone(), 0, |_| Ok(1)).unwrap();

    memo.query(identity.clone(), 1, |memo| {
        Ok(*memo.query(identity.clone(), 0, |_| panic!("dependency is cached"))? + 1)
    })
    .unwrap();

    let parent = memo
        .query(identity.clone(), 2, |memo| {
            Ok(*memo.query(identity.clone(), 1, |_| panic!("intermediate is cached"))? + 1)
        })
        .unwrap();

    assert_eq!(*parent, 3);
    assert_eq!(memo.len(), 4);

    memo.invalidate(&identity, &0);
    assert_eq!(memo.len(), 1);

    let retained = memo
        .query(identity.clone(), 9, |_| {
            panic!("independent query remains cached")
        })
        .unwrap();

    assert!(Arc::ptr_eq(&independent, &retained));

    let recomputed = memo
        .query(identity.clone(), 2, |memo| {
            Ok(*memo.query(identity.clone(), 1, |memo| {
                Ok(*memo.query(identity.clone(), 0, |_| Ok(10))? + 1)
            })? + 1)
        })
        .unwrap();

    assert_eq!(*recomputed, 12);
    assert!(!Arc::ptr_eq(&parent, &recomputed));
    assert_eq!(*parent, 3);
}

#[test]
fn eviction_removes_dependents_and_releases_results() {
    let identity = parse(b"return 1").root().identity();
    let mut memo = Memo::new(3);
    let child = memo.query(identity.clone(), 0, |_| Ok(10)).unwrap();
    let weak = Arc::downgrade(&child);

    let parent = memo
        .query(identity.clone(), 1, |memo| {
            Ok(*memo.query(identity.clone(), 0, |_| panic!("dependency is cached"))? + 1)
        })
        .unwrap();

    drop(child);
    memo.query(identity.clone(), 2, |_| Ok(20)).unwrap();
    memo.query(identity.clone(), 3, |_| Ok(30)).unwrap();
    assert_eq!(memo.len(), 2);
    assert!(weak.upgrade().is_none());
    assert_eq!(*parent, 11);

    let rebuilt = memo
        .query(identity.clone(), 1, |memo| {
            Ok(*memo.query(identity.clone(), 0, |_| Ok(100))? + 1)
        })
        .unwrap();

    assert_eq!(*rebuilt, 101);
    assert!(memo.len() <= 3);
    memo.invalidate(&identity, &0);
    assert_eq!(memo.len(), 1);

    assert_eq!(
        *memo
            .query(identity.clone(), 3, |_| panic!("unrelated query survives"))
            .unwrap(),
        30
    );

    memo.clear();
    assert!(memo.is_empty());

    for environment in 0..100 {
        memo.query(identity.clone(), environment, |_| Ok(environment))
            .unwrap();

        assert!(memo.len() <= 3);
    }
}

#[test]
fn cycles_and_capacity_errors_leave_no_active_queries() {
    let identity = parse(b"return 1").root().identity();
    let mut memo = Memo::<_, usize>::new(4);

    let cycle = memo.query(identity.clone(), 0, |memo| {
        Ok(*memo.query(identity.clone(), 1, |memo| {
            Ok(*memo.query(identity.clone(), 0, |_| Ok(1))?)
        })?)
    });

    assert_eq!(cycle, Err(AnalysisError::Cycle));
    assert!(memo.is_empty());
    assert_eq!(*memo.query(identity.clone(), 0, |_| Ok(5)).unwrap(), 5);

    let mut limited = Memo::<_, usize>::new(1);

    let exhausted = limited.query(identity.clone(), 0, |memo| {
        Ok(*memo.query(identity.clone(), 1, |_| Ok(1))?)
    });

    assert_eq!(exhausted, Err(AnalysisError::Capacity));
    assert!(limited.is_empty());
    assert_eq!(*limited.query(identity.clone(), 0, |_| Ok(6)).unwrap(), 6);

    assert_eq!(
        Memo::new(0).query(identity, 0, |_| Ok(1)),
        Err(AnalysisError::Capacity)
    );
}

#[test]
fn invalidation_during_computation_never_publishes_a_stale_result() {
    let identity = parse(b"return 1").root().identity();
    let mut memo = Memo::new(4);

    let result = memo.query(identity.clone(), 0, |memo| {
        let dependency = memo.query(identity.clone(), 1, |_| Ok(10))?;
        memo.invalidate(&identity, &1);

        Ok(*dependency + 1)
    });

    assert_eq!(result, Err(AnalysisError::Invalidated));
    assert!(memo.is_empty());

    let result = memo.query(identity.clone(), 0, |memo| {
        memo.clear();

        Ok(10)
    });

    assert_eq!(result, Err(AnalysisError::Invalidated));
    assert!(memo.is_empty());
    assert_eq!(*memo.query(identity, 0, |_| Ok(20)).unwrap(), 20);
}

#[test]
fn eviction_during_computation_does_not_cache_incomplete_dependencies() {
    let identity = parse(b"return 1").root().identity();
    let mut memo = Memo::new(2);

    let result = memo.query(identity.clone(), 0, |memo| {
        let first = memo.query(identity.clone(), 1, |_| Ok(10))?;
        let second = memo.query(identity.clone(), 2, |_| Ok(20))?;

        Ok(*first + *second)
    });

    assert_eq!(result, Err(AnalysisError::Invalidated));
    assert_eq!(memo.len(), 1);
    assert_eq!(*memo.query(identity, 0, |_| Ok(40)).unwrap(), 40);
}
