import { Component, Singleton } from "@anvil-di/anvil";
import { Parent } from "./parent";
import { ReportService } from "./report-service";

@Singleton
@Component({ modules: [] })
export abstract class AppComponent {
  abstract reports(): ReportService;
  abstract parent(): Parent;
}
