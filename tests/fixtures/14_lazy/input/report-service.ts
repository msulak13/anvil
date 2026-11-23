import { Inject, Lazy } from "@anvil-di/anvil";
import { PdfExporter } from "./pdf-exporter";

// The exporter is only built when a report is actually exported.
@Inject
export class ReportService {
  constructor(private exporter: Lazy<PdfExporter>) {}

  export(): PdfExporter {
    return this.exporter.get();
  }
}
