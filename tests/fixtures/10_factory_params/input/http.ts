// Toy HTTP shapes used as factory-param types. Real Express types
// would arrive from the `express` package; this fixture keeps the
// runtime stub-free by defining its own.
export interface HttpRequest {
  url: string;
  headers: Record<string, string>;
}

export interface HttpResponse {
  send(status: number, body: string): void;
}
