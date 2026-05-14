//! IR snapshot tests for the decorator extractor.
//!
//! Each test feeds a small TypeScript source through `parse_source` and
//! snapshots the resulting [`anvil_core::ir::ParsedFile`] via `insta`. Update
//! snapshots after intentional IR changes with `cargo insta review`.

use anvil_parser::parse_source;
use anvil_parser::{decorators::ExtractError, ParseError};
use insta::assert_debug_snapshot;

#[test]
fn module_with_provides() {
    let src = r#"
        import { Module, Provides } from "@anvil-di/anvil";
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
        import { Inject } from "@anvil-di/anvil";
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
        import { Inject, Singleton } from "@anvil-di/anvil";

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
        import { Component, Singleton } from "@anvil-di/anvil";
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
        import { Inject, Module, Provides, Component, Singleton } from "@anvil-di/anvil";

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
        import { Module, Provides } from "@anvil-di/anvil";
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
        import { Module, Provides } from "@anvil-di/anvil";

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
        import { Inject } from "@anvil-di/anvil";
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
        import { Module, Binds } from "@anvil-di/anvil";
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
        import { Module, Binds } from "@anvil-di/anvil";
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
        import { Module, Binds } from "@anvil-di/anvil";
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
        import { Module, Binds } from "@anvil-di/anvil";
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
        import { Component } from "@anvil-di/anvil";

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

#[test]
fn async_provides_method_unwraps_promise_to_inner_key() {
    // M12: an async @Provides has return type Promise<T>; the parser
    // unwraps the Promise wrapper for the binding key (so consumers
    // see the resolved type) and sets `is_async: true` on the provider.
    let src = r#"
        import { Module, Provides, Singleton } from "@anvil-di/anvil";
        import { Pool } from "./pool";
        import { Config } from "./config";

        @Module
        export class DatabaseModule {
            @Singleton
            @Provides
            static async pool(config: Config): Promise<Pool> {
                return null as any;
            }
        }
    "#;
    let parsed = parse_source(src, "database-module.ts").unwrap();
    assert_debug_snapshot!(parsed);
}

#[test]
fn async_provides_without_promise_return_is_rejected() {
    // An `async` method that doesn't return Promise<T> is technically
    // valid TS (the async keyword wraps the value in Promise.resolve)
    // but anvil requires an explicit Promise<T> annotation so the
    // unwrap is unambiguous.
    let src = r#"
        import { Module, Provides } from "@anvil-di/anvil";
        import { Pool } from "./pool";

        @Module
        export class M {
            @Provides static async pool(): Pool { return null as any; }
        }
    "#;
    let err = parse_source(src, "m.ts").unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("Promise<T>"), "unexpected error: {msg}");
}

// `async constructor` is a syntax error per the JS spec; Oxc rejects
// it before anvil-parser's extractor runs, so the
// `ExtractError::AsyncInjectCtor` diagnostic exists as
// defense-in-depth (in case a future toolchain loosens the parser
// rule) but isn't reachable today and has no test.

#[test]
fn module_with_into_set_provides() {
    // Two @IntoSet @Provides contributions to Set<Plugin>. The parser
    // emits raw bindings with role: IntoSet — graph aggregation folds
    // them into a synthesized Provider::SetMultibinding later.
    let src = r#"
        import { Module, Provides, IntoSet } from "@anvil-di/anvil";
        import { Plugin } from "./plugin";
        import { AuthPlugin } from "./auth-plugin";
        import { LoggingPlugin } from "./logging-plugin";

        @Module
        export class PluginsModule {
            @IntoSet @Provides static auth(): Plugin { return new AuthPlugin(); }
            @IntoSet @Provides static logging(): Plugin { return new LoggingPlugin(); }
        }
    "#;
    let parsed = parse_source(src, "plugins-module.ts").unwrap();
    assert_debug_snapshot!(parsed);
}

#[test]
fn into_set_on_binds_is_rejected() {
    let src = r#"
        import { Module, Binds, IntoSet } from "@anvil-di/anvil";
        import { Plugin } from "./plugin";
        import { AuthPlugin } from "./auth-plugin";

        @Module
        export class PluginsModule {
            @IntoSet @Binds static authImpl(impl: AuthPlugin): Plugin { return impl; }
        }
    "#;
    let err = parse_source(src, "plugins-module.ts").unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("@IntoSet") && msg.contains("@Provides"),
        "unexpected error: {msg}"
    );
}

#[test]
fn entry_point_with_set_return_type_parses() {
    // A `Set<Plugin>` return on an entry point lowers to Key::Set.
    let src = r#"
        import { Component } from "@anvil-di/anvil";
        import { Plugin } from "./plugin";

        @Component({ modules: [] })
        export abstract class App {
            abstract plugins(): Set<Plugin>;
        }
    "#;
    let parsed = parse_source(src, "app.ts").unwrap();
    assert_debug_snapshot!(parsed);
}

#[test]
fn subcomponent_factory_with_typed_parameters() {
    // M11: a parent @Component exposes a @Subcomponent through an
    // abstract method that takes runtime parameters. The parser must
    // capture each parameter as a FactoryParam (name + key + span)
    // alongside the return-type Key. Regular @Component entry points
    // remain zero-arg; the graph layer is what rejects parameters
    // declared on a non-subcomponent factory.
    let src = r#"
        import { Component, Subcomponent } from "@anvil-di/anvil";

        export interface HttpRequest { url: string }
        export interface HttpResponse { send(body: string): void }

        @Subcomponent({ modules: [] })
        export abstract class RequestComponent {
            abstract handler(): RequestHandler;
        }

        @Component({ modules: [] })
        export abstract class App {
            abstract requestComponent(req: HttpRequest, res: HttpResponse): RequestComponent;
        }

        export class RequestHandler {}
    "#;
    let parsed = parse_source(src, "app.ts").unwrap();
    assert_debug_snapshot!(parsed);
}

#[test]
fn subcomponent_with_modules_and_entry_points() {
    let src = r#"
        import { Component, Subcomponent } from "@anvil-di/anvil";

        @Subcomponent({ modules: [] })
        export abstract class RequestComponent {
            abstract handler(): RequestHandler;
        }

        @Component({ modules: [] })
        export abstract class App {
            abstract requestComponent(): RequestComponent;
        }

        export class RequestHandler {}
    "#;
    let parsed = parse_source(src, "app.ts").unwrap();
    assert_debug_snapshot!(parsed);
}

#[test]
fn token_provides_extracts_token_key() {
    // M14: @Provides whose return type is Token<T, "name"> lowers to
    // Key::Token { name }. The inner type string is stored in
    // token_inner_type on the Binding for codegen use.
    let src = r#"
        import { Module, Provides } from "@anvil-di/anvil";
        import { Token } from "@anvil-di/anvil";
        import { Database } from "./database";

        @Module
        export class DbModule {
            @Provides
            static primaryDb(): Token<Database, "primary-db"> {
                return null as any;
            }
        }
    "#;
    let parsed = parse_source(src, "db-module.ts").unwrap();
    assert_debug_snapshot!(parsed);
}

#[test]
fn token_inject_param_uses_token_key() {
    // M14: a constructor parameter typed Token<T, "name"> becomes a dep
    // with Key::Token { name } so the dagger can wire it from the
    // corresponding @Provides binding.
    let src = r#"
        import { Inject } from "@anvil-di/anvil";
        import { Token } from "@anvil-di/anvil";
        import { Database } from "./database";

        @Inject
        export class Service {
            constructor(private db: Token<Database, "primary-db">) {}
        }
    "#;
    let parsed = parse_source(src, "service.ts").unwrap();
    assert_debug_snapshot!(parsed);
}

#[test]
fn generic_provides_extracts_key_with_type_args() {
    // M15: @Provides returning Repository<User> lowers to
    // Key::Class { name: "Repository", type_args: [Key::Class { name: "User" }] }.
    let src = r#"
        import { Module, Provides } from "@anvil-di/anvil";
        import { Repository } from "./repository";
        import { User } from "./user";

        @Module
        export class UserModule {
            @Provides
            static users(): Repository<User> {
                return new Repository();
            }
        }
    "#;
    let parsed = parse_source(src, "user-module.ts").unwrap();
    assert_debug_snapshot!(parsed);
}

#[test]
fn generic_inject_param_uses_key_with_type_args() {
    // M15: a constructor parameter typed Repository<User> becomes a dep
    // with Key::Class { name: "Repository", type_args: [User] }.
    let src = r#"
        import { Inject } from "@anvil-di/anvil";
        import { Repository } from "./repository";
        import { User } from "./user";

        @Inject
        export class UserService {
            constructor(private repo: Repository<User>) {}
        }
    "#;
    let parsed = parse_source(src, "user-service.ts").unwrap();
    assert_debug_snapshot!(parsed);
}

#[test]
fn generic_with_primitive_type_arg_produces_distinct_keys() {
    // Bug 2 fix: Repository<string> and Repository<number> must produce
    // *different* keys. Previously ts_type_to_key returned Err for keyword
    // types, .ok() swallowed it, and both collapsed to Key::Class { type_args: [] }.
    let src = r#"
        import { Inject } from "@anvil-di/anvil";
        import { Repository } from "./repository";

        @Inject
        export class DataService {
            constructor(
                private names: Repository<string>,
                private counts: Repository<number>,
            ) {}
        }
    "#;
    let parsed = parse_source(src, "data-service.ts").unwrap();
    // The two deps must have different keys.
    let inject = parsed.inject_classes.first().unwrap();
    assert_eq!(inject.deps.len(), 2);
    assert_ne!(
        inject.deps[0], inject.deps[1],
        "Repository<string> and Repository<number> must be distinct keys",
    );
}

#[test]
fn token_non_literal_name_is_rejected() {
    // Bug 1 fix: Token<T, MY_CONST> (non-literal second arg) must error rather than
    // silently producing Key::Class { name: "Token", type_args: [...] }.
    let src = r#"
        import { Module, Provides } from "@anvil-di/anvil";
        import type { Token } from "@anvil-di/anvil";
        import { Database } from "./database";
        const MY_TOKEN = "primary-db";

        @Module
        export class DbModule {
            @Provides
            static db(): Token<Database, typeof MY_TOKEN> {
                return null as any;
            }
        }
    "#;
    let err = parse_source(src, "db-module.ts").unwrap_err();
    assert!(
        matches!(
            err,
            ParseError::Extract(ExtractError::TokenNonLiteralName { .. })
        ),
        "expected TokenNonLiteralName, got: {err:?}",
    );
}
