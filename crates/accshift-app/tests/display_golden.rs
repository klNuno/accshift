//! Golden display lists of the demo scenario on the Steam tab, against the
//! Tauri reference captures (`spike-main`: hover, folder and search shots).
//! The accounts load through `AccountLoading` and the job pool over the
//! mock, as the shell does.

mod support;

use accshift_app::accounts::loader::LoadOptions;
use accshift_app::accounts::loading::{AccountLoading, LoadingContext};
use accshift_app::accounts::profile_cache::{ProfileCaches, UrlPolicy};
use accshift_app::display::{
    build_display_view, CardContext, DisplayBody, DisplayItem, DisplayLists, DisplayPipeline,
    DisplayView, DragPreview, FolderNavigation, PipelineInput,
};
use accshift_app::jobs::Jobs;
use accshift_app::platforms::{PlatformRegistry, RuntimeOs};
use accshift_app::storage;
use accshift_app::stores::{ClientStores, StoreContext};
use accshift_app::time::Clock;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

const TAB: &str = "steam";
const SMURFS: &str = "demo-folder-smurfs";

struct Demo {
    jobs: Jobs,
    registry: PlatformRegistry,
    stores: ClientStores,
    loading: AccountLoading,
    clock: Clock,
}

impl Demo {
    fn load() -> Self {
        let backend = support::mock("demo");
        let (storage, report) = storage::initialize(&backend, None, None, true);
        assert!(report.load_error.is_none(), "{:?}", report.load_error);
        let stores = ClientStores::new(storage, StoreContext::shipped());
        let clock = Clock::mock(1_791_000_000);
        let caches = Arc::new(ProfileCaches::new(UrlPolicy::HttpOrFile, clock.now_ms()));
        let mut demo = Demo {
            jobs: Jobs::new(Arc::new(backend), 2, || {}),
            registry: PlatformRegistry::shipped(),
            stores,
            loading: AccountLoading::new(Some(TAB.to_string()), caches),
            clock,
        };
        assert!(demo.with_ctx(&[], |loading, ctx| loading
            .load_accounts(ctx, LoadOptions::default())));
        let navigation = root_navigation(&demo);
        demo.settle(&navigation);
        assert!(!demo.loading.loader.loading());
        demo
    }

    fn with_ctx<R>(
        &mut self,
        visible: &[String],
        run: impl FnOnce(&mut AccountLoading, &mut LoadingContext<'_>) -> R,
    ) -> R {
        let mut ctx = LoadingContext {
            jobs: &self.jobs,
            registry: &self.registry,
            stores: &mut self.stores,
            active_tab: TAB,
            runtime_os: RuntimeOs::Windows,
            visible_ids: visible,
            is_searching: false,
            clock: &self.clock,
            now: Instant::now(),
        };
        run(&mut self.loading, &mut ctx)
    }

    /// The shell's frame loop until every job, deferred step and priming
    /// timer landed: poll, recompute the grid, track what it shows.
    fn settle(&mut self, navigation: &FolderNavigation) {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut pipeline = DisplayPipeline::new();
        let mut visible: Vec<String> = Vec::new();
        loop {
            self.with_ctx(&visible, |loading, ctx| loading.poll(ctx));
            visible = self.view(navigation, &mut pipeline).0.visible_account_ids;
            self.with_ctx(&visible, |loading, ctx| loading.track_priming(ctx, false));
            if !self.loading.has_pending_work() && self.loading.priming.next_deadline().is_none() {
                break;
            }
            assert!(Instant::now() < deadline, "jobs did not settle");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn view(
        &self,
        navigation: &FolderNavigation,
        pipeline: &mut DisplayPipeline,
    ) -> (DisplayLists, DisplayView) {
        let lists = pipeline.compute(&PipelineInput {
            navigation,
            drag: DragPreview::default(),
            accounts: self.loading.loader.accounts(),
            pending_setup_account: None,
            expanded_folders: false,
            active_tab: TAB,
            stores: &self.stores,
        });
        let adapter = self.registry.adapter(TAB);
        let collapsed = BTreeSet::new();
        let view = build_display_view(
            &CardContext {
                loader: &self.loading.loader,
                stores: &self.stores,
                navigation,
                platform: self.registry.definition(TAB),
                adapter: adapter.as_deref(),
                active_tab: TAB,
                pending_setup_account: None,
                bulk_selection: None,
                drag_item: None,
                collapsed_sections: &collapsed,
            },
            &lists,
        );
        (lists, view)
    }
}

fn root_navigation(demo: &Demo) -> FolderNavigation {
    let mut navigation = FolderNavigation::new();
    navigation.refresh_current_items(&demo.stores, TAB);
    navigation
}

#[test]
fn golden_root_matches_the_tauri_grid() {
    let demo = Demo::load();
    let navigation = root_navigation(&demo);
    let mut pipeline = DisplayPipeline::new();
    let (lists, view) = demo.view(&navigation, &mut pipeline);
    assert_eq!(
        view.labels(),
        [
            "Smurfs",
            "main",
            "bro's account",
            "alt",
            "trading",
            "faceit",
            "ranked grind",
            "chill",
            "lan party",
            "streams",
            "esea",
            "tournaments",
            "family",
            "practice",
        ]
    );
    assert_eq!(lists.rendered_account_count, 17);

    // The background refresh after the load filled the avatars from the
    // mock files; the card colours come from the demo store.
    let cards: Vec<_> = view
        .items()
        .iter()
        .filter_map(|item| match item {
            DisplayItem::Account(card) => Some(card),
            _ => None,
        })
        .collect();
    assert_eq!(cards.len(), 13);
    let main = cards[0];
    assert_eq!(main.card_color, "#8b5cf6");
    assert!(main.active, "the demo's current account is main");
    let with_avatar = cards
        .iter()
        .filter(|card| card.avatar.url.is_some())
        .count();
    assert_eq!(with_avatar, cards.len(), "every visible card has an avatar");
    assert!(cards
        .iter()
        .all(|card| !card.avatar.loading && !card.avatar.pending));
    match &view.items()[0] {
        DisplayItem::Folder(folder) => assert_eq!(folder.folder.id, SMURFS),
        other => panic!("expected the Smurfs folder first, got {other:?}"),
    }
}

#[test]
fn golden_smurfs_folder_matches_the_tauri_grid() {
    let demo = Demo::load();
    let mut navigation = FolderNavigation::new();
    navigation.set_current_folder_id(Some(SMURFS.to_string()));
    navigation.refresh_current_items(&demo.stores, TAB);
    let mut pipeline = DisplayPipeline::new();
    let (_, view) = demo.view(&navigation, &mut pipeline);
    assert_eq!(
        view.labels(),
        ["Back", "smurf 1", "smurf 2", "smurf 3", "smurf 4"]
    );
    assert!(matches!(
        view.body,
        DisplayBody::Items {
            folder_empty: false,
            ..
        }
    ));
}

#[test]
fn golden_search_ra_matches_the_tauri_grid() {
    let demo = Demo::load();
    let mut navigation = root_navigation(&demo);
    let mut pipeline = DisplayPipeline::new();
    let start = Instant::now();
    navigation.set_search_query("ra");
    pipeline.set_query("ra", start);
    // Inside the 80 ms debounce the grid still shows the root.
    assert!(!pipeline.tick(start));
    assert!(pipeline.tick(start + Duration::from_millis(80)));
    let (lists, view) = demo.view(&navigation, &mut pipeline);
    assert!(lists.search_layout);
    assert_eq!(view.labels(), ["trading", "ranked grind", "practice"]);
}
