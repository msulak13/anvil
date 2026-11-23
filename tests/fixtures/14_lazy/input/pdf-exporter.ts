import { Inject } from "@anvil-di/anvil";

@Inject
export class PdfExporter {
  static constructed = 0;

  constructor() {
    PdfExporter.constructed++;
  }
}
