// Pretend connection pool. The real-world version would await
// `mysql.createPool(...)` or similar — this fixture stays self-
// contained.
export class Database {
  constructor(public readonly url: string) {}

  query(sql: string): string {
    return `${this.url}> ${sql}`;
  }
}
