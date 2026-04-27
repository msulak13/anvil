//! IR snapshot tests for the decorator extractor.
//!
//! Each test feeds a small TypeScript source through `parse_source` and
//! snapshots the resulting [`tsdi_core::ir::ParsedFile`] via `insta`. Update
//! snapshots after intentional IR changes with `cargo insta review`.

use insta::assert_debug_snapshot;
use tsdi_parser::parse_source;

#[test]
fn module_with_provides() {
    let src = r#"
        import { Module, Provides } from "tsdi";
        import { Pump } from "./pump";

        @Module
        export class CoffeeModule {
            @Provides static providePump(): Pump { return new Pump(); }
        }
    "#;
    let parsed = parse_source(src, "coffee_module.ts").unwrap();
    assert_debug_snapshot!(parsed);
}

#[test]
fn inject_ctor_with_dep() {
    let src = r#"
        import { Inject } from "tsdi";
        import { Heater } from "./heater";

        @Inject
        export class Pump {
            constructor(private heater: Heater) {}
        }
    "#;
    let parsed = parse_source(src, "pump.ts").unwrap();
    assert_debug_snapshot!(parsed);
}

#[test]
fn singleton_class_with_inject_ctor() {
    let src = r#"
        import { Inject, Singleton } from "tsdi";

        @Inject
        @Singleton
        export class Heater {
            constructor() {}
        }
    "#;
    let parsed = parse_source(src, "heater.ts").unwrap();
    assert_debug_snapshot!(parsed);
}

#[test]
fn component_with_modules_and_entry_points() {
    let src = r#"
        import { Component, Singleton } from "tsdi";
        import { CoffeeModule } from "./coffee_module";
        import { Pump } from "./pump";
        import { Heater } from "./heater";

        @Singleton
        @Component({ modules: [CoffeeModule] })
        export abstract class CoffeeShop {
            abstract pump(): Pump;
            abstract heater(): Heater;
        }
    "#;
    let parsed = parse_source(src, "coffee_shop.ts").unwrap();
    assert_debug_snapshot!(parsed);
}

#[test]
fn full_coffee_example_in_one_file() {
    // Same-file `Heater` reference exercises the SAME_FILE sentinel.
    let src = r#"
        import { Inject, Module, Provides, Component, Singleton } from "tsdi";

        @Inject
        @Singleton
        export class Heater {
            constructor() {}
        }

        @Inject
        export class Pump {
            constructor(private heater: Heater) {}
        }

        @Module
        export class CoffeeModule {
            @Provides static provideTimer(): Heater { return new Heater(); }
        }

        @Component({ modules: [CoffeeModule] })
        export abstract class CoffeeShop {
            abstract pump(): Pump;
        }
    "#;
    let parsed = parse_source(src, "coffee.ts").unwrap();
    assert_debug_snapshot!(parsed);
}

#[test]
fn provides_must_be_static() {
    let src = r#"
        import { Module, Provides } from "tsdi";
        import { Pump } from "./pump";

        @Module
        export class BadModule {
            @Provides providePump(): Pump { return new Pump(); }
        }
    "#;
    let err = parse_source(src, "bad_module.ts").unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("must be a static method"),
        "unexpected error: {msg}"
    );
}

#[test]
fn provides_requires_return_type() {
    let src = r#"
        import { Module, Provides } from "tsdi";

        @Module
        export class BadModule {
            @Provides static providePump() { return 42; }
        }
    "#;
    let err = parse_source(src, "bad_module.ts").unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("must declare an explicit return type"),
        "unexpected error: {msg}"
    );
}

#[test]
fn legacy_inject_on_constructor_is_rejected() {
    let src = r#"
        import { Inject } from "tsdi";
        export class Pump { @Inject constructor() {} }
    "#;
    let err = parse_source(src, "pump.ts").unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("@Inject must be applied to the class"),
        "unexpected error: {msg}"
    );
}

#[test]
fn module_with_binds_method() {
    let src = r#"
        import { Module, Binds } from "tsdi";
        import { Heater } from "./heater";
        import { ElectricHeater } from "./electric_heater";

        @Module
        export class HeaterModule {
            @Binds static bindHeater(impl: ElectricHeater): Heater { return impl; }
        }
    "#;
    let parsed = parse_source(src, "heater_module.ts").unwrap();
    assert_debug_snapshot!(parsed);
}

#[test]
fn binds_must_be_static() {
    let src = r#"
        import { Module, Binds } from "tsdi";
        import { Heater } from "./heater";
        import { ElectricHeater } from "./electric_heater";

        @Module
        export class HeaterModule {
            @Binds bindHeater(impl: ElectricHeater): Heater { return impl; }
        }
    "#;
    let err = parse_source(src, "heater_module.ts").unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("must be a static method"),
        "unexpected error: {msg}"
    );
}

#[test]
fn binds_requires_return_type() {
    let src = r#"
        import { Module, Binds } from "tsdi";
        import { ElectricHeater } from "./electric_heater";

        @Module
        export class HeaterModule {
            @Binds static bindHeater(impl: ElectricHeater) { return impl; }
        }
    "#;
    let err = parse_source(src, "heater_module.ts").unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("must declare an explicit return type"),
        "unexpected error: {msg}"
    );
}

#[test]
fn binds_requires_exactly_one_parameter() {
    let src = r#"
        import { Module, Binds } from "tsdi";
        import { Heater } from "./heater";

        @Module
        export class HeaterModule {
            @Binds static bindHeater(): Heater { return null as any; }
        }
    "#;
    let err = parse_source(src, "heater_module.ts").unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("exactly one parameter"),
        "unexpected error: {msg}"
    );
}

#[test]
fn entry_point_requires_return_type() {
    let src = r#"
        import { Component } from "tsdi";

        @Component({ modules: [] })
        export abstract class Shop {
            abstract pump();
        }
    "#;
    let err = parse_source(src, "shop.ts").unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("entry point") && msg.contains("explicit return type"),
        "unexpected error: {msg}"
    );
}
