//! Snapshot tests for the M4 emitter.
//!
//! IR is constructed by hand (not via the parser) so this crate's tests
//! depend only on `tsdi-core`. Paths are taken from a real `TempDir` so
//! they're truly absolute on both Unix and Windows; the emitted output
//! contains only *relative* specifiers, so the snapshots are
//! platform-portable.

use std::path::PathBuf;

use tempfile::TempDir;
use tsdi_core::ir::{
    Binding, ClassRef, ComponentDecl, EntryPoint, Key, ModuleDecl, ModulePath, Provider, Scope,
    SourceSpan,
};

use tsdi_codegen::emit_component;

fn span_of(path: &str) -> SourceSpan {
    SourceSpan::new(path, 0, 0)
}

fn class_key(module: &str, name: &str) -> Key {
    Key::Class {
        module: ModulePath(module.into()),
        name: name.into(),
    }
}

fn class_ref(module: &str, name: &str) -> ClassRef {
    ClassRef {
        module: ModulePath(module.into()),
        name: name.into(),
    }
}

#[test]
fn emit_simple_provides_module() {
    // src/coffee/coffee-component.ts has @Component
    // src/coffee/coffee-module.ts has @Module providing Heater + Pump (Pump deps on Heater)
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    let comp_path: String = root
        .join("coffee-component.ts")
        .to_string_lossy()
        .into_owned();
    let mod_path: String = root.join("coffee-module.ts").to_string_lossy().into_owned();

    let heater_key = class_key(&mod_path, "Heater");
    let pump_key = class_key(&mod_path, "Pump");

    let module = ModuleDecl {
        class: class_ref(&mod_path, "CoffeeModule"),
        provides: vec![
            Binding {
                key: heater_key.clone(),
                provider: Provider::ProvidesMethod {
                    module: class_ref(&mod_path, "CoffeeModule"),
                    method: "heater".into(),
                },
                scope: Scope::Unscoped,
                deps: vec![],
                source: span_of(&mod_path),
            },
            Binding {
                key: pump_key.clone(),
                provider: Provider::ProvidesMethod {
                    module: class_ref(&mod_path, "CoffeeModule"),
                    method: "pump".into(),
                },
                scope: Scope::Unscoped,
                deps: vec![heater_key.clone()],
                source: span_of(&mod_path),
            },
        ],
        source: span_of(&mod_path),
    };

    let component = ComponentDecl {
        class: class_ref(&comp_path, "CoffeeShop"),
        modules: vec![class_ref(&mod_path, "CoffeeModule")],
        scope: Scope::Unscoped,
        entry_points: vec![
            EntryPoint {
                name: "pump".into(),
                key: pump_key,
                source: span_of(&comp_path),
            },
            EntryPoint {
                name: "heater".into(),
                key: heater_key,
                source: span_of(&comp_path),
            },
        ],
        source: span_of(&comp_path),
    };

    let out = emit_component(&component, &[module], &[], "0.0.1").unwrap();
    insta::assert_snapshot!(out);
}

#[test]
fn emit_inject_ctor_chain() {
    // Heater has @Inject ctor with no deps; Pump has @Inject ctor with Heater dep.
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    let comp_path: String = root
        .join("shop-component.ts")
        .to_string_lossy()
        .into_owned();
    let heater_path: String = root.join("heater.ts").to_string_lossy().into_owned();
    let pump_path: String = root.join("pump.ts").to_string_lossy().into_owned();

    let heater_key = class_key(&heater_path, "Heater");
    let pump_key = class_key(&pump_path, "Pump");

    let inject_classes = vec![
        Binding {
            key: heater_key.clone(),
            provider: Provider::InjectCtor {
                class: class_ref(&heater_path, "Heater"),
            },
            scope: Scope::Unscoped,
            deps: vec![],
            source: span_of(&heater_path),
        },
        Binding {
            key: pump_key.clone(),
            provider: Provider::InjectCtor {
                class: class_ref(&pump_path, "Pump"),
            },
            scope: Scope::Unscoped,
            deps: vec![heater_key.clone()],
            source: span_of(&pump_path),
        },
    ];

    let component = ComponentDecl {
        class: class_ref(&comp_path, "Shop"),
        modules: vec![],
        scope: Scope::Unscoped,
        entry_points: vec![EntryPoint {
            name: "pump".into(),
            key: pump_key,
            source: span_of(&comp_path),
        }],
        source: span_of(&comp_path),
    };

    let out = emit_component(&component, &[], &inject_classes, "0.0.1").unwrap();
    insta::assert_snapshot!(out);
}

#[test]
fn emit_singleton_caches_via_lazy_field() {
    // Singleton component with a singleton Heater dep + an unscoped Pump
    // that depends on Heater. Heater should get a private cache field +
    // `??=` factory; Pump should be a plain `return new Pump(...)`.
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    let comp_path: String = root
        .join("shop-component.ts")
        .to_string_lossy()
        .into_owned();
    let heater_path: String = root.join("heater.ts").to_string_lossy().into_owned();
    let pump_path: String = root.join("pump.ts").to_string_lossy().into_owned();

    let heater_key = class_key(&heater_path, "Heater");
    let pump_key = class_key(&pump_path, "Pump");

    let inject_classes = vec![
        Binding {
            key: heater_key.clone(),
            provider: Provider::InjectCtor {
                class: class_ref(&heater_path, "Heater"),
            },
            scope: Scope::Singleton,
            deps: vec![],
            source: span_of(&heater_path),
        },
        Binding {
            key: pump_key.clone(),
            provider: Provider::InjectCtor {
                class: class_ref(&pump_path, "Pump"),
            },
            scope: Scope::Unscoped,
            deps: vec![heater_key.clone()],
            source: span_of(&pump_path),
        },
    ];

    let component = ComponentDecl {
        class: class_ref(&comp_path, "Shop"),
        modules: vec![],
        scope: Scope::Singleton,
        entry_points: vec![
            EntryPoint {
                name: "pump".into(),
                key: pump_key,
                source: span_of(&comp_path),
            },
            EntryPoint {
                name: "heater".into(),
                key: heater_key,
                source: span_of(&comp_path),
            },
        ],
        source: span_of(&comp_path),
    };

    let out = emit_component(&component, &[], &inject_classes, "0.0.1").unwrap();
    insta::assert_snapshot!(out);
}

#[test]
fn emit_binds_alias_delegates_to_target() {
    // @Module exposes a @Binds method that aliases ElectricHeater to Heater.
    // ElectricHeater has an @Inject ctor (no deps). Component requests Heater.
    // Codegen should emit: getHeater() returns this.getElectricHeater().
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    let comp_path: String = root
        .join("shop-component.ts")
        .to_string_lossy()
        .into_owned();
    let mod_path: String = root.join("heater-module.ts").to_string_lossy().into_owned();
    let heater_path: String = root.join("heater.ts").to_string_lossy().into_owned();
    let electric_path: String = root
        .join("electric-heater.ts")
        .to_string_lossy()
        .into_owned();

    let heater_key = class_key(&heater_path, "Heater");
    let electric_key = class_key(&electric_path, "ElectricHeater");

    let module = ModuleDecl {
        class: class_ref(&mod_path, "HeaterModule"),
        provides: vec![Binding {
            key: heater_key.clone(),
            provider: Provider::Binds {
                target: electric_key.clone(),
            },
            scope: Scope::Unscoped,
            deps: vec![electric_key.clone()],
            source: span_of(&mod_path),
        }],
        source: span_of(&mod_path),
    };

    let inject_classes = vec![Binding {
        key: electric_key.clone(),
        provider: Provider::InjectCtor {
            class: class_ref(&electric_path, "ElectricHeater"),
        },
        scope: Scope::Unscoped,
        deps: vec![],
        source: span_of(&electric_path),
    }];

    let component = ComponentDecl {
        class: class_ref(&comp_path, "Shop"),
        modules: vec![class_ref(&mod_path, "HeaterModule")],
        scope: Scope::Unscoped,
        entry_points: vec![EntryPoint {
            name: "heater".into(),
            key: heater_key,
            source: span_of(&comp_path),
        }],
        source: span_of(&comp_path),
    };

    let out = emit_component(&component, &[module], &inject_classes, "0.0.1").unwrap();
    insta::assert_snapshot!(out);
}

#[test]
fn validation_failure_short_circuits() {
    // Missing binding: Pump entry-point with no provider for Pump.
    let tmp = TempDir::new().unwrap();
    let root: PathBuf = tmp.path().to_path_buf();
    let comp_path: String = root.join("c.ts").to_string_lossy().into_owned();
    let pump_path: String = root.join("pump.ts").to_string_lossy().into_owned();
    let pump_key = class_key(&pump_path, "Pump");

    let component = ComponentDecl {
        class: class_ref(&comp_path, "Shop"),
        modules: vec![],
        scope: Scope::Unscoped,
        entry_points: vec![EntryPoint {
            name: "pump".into(),
            key: pump_key,
            source: span_of(&comp_path),
        }],
        source: span_of(&comp_path),
    };

    let err = emit_component(&component, &[], &[], "0.0.1").unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("validation") || msg.contains("diagnostic"),
        "got: {msg}"
    );
}
