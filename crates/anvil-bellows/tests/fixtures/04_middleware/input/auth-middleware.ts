export function requireAuth(req: unknown, res: unknown, next: () => void): void {
  next();
}

export function requireAdmin(req: unknown, res: unknown, next: () => void): void {
  next();
}
