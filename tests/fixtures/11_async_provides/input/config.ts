// Configuration object — built async because in real apps you'd be
// reading it from disk or pulling from a secrets manager.
export class Config {
  constructor(
    public readonly databaseUrl: string,
    public readonly cacheTtlMs: number,
  ) {}
}
