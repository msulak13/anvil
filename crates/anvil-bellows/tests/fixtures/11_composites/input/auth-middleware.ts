export function requireAuth(req: unknown, res: unknown, next: () => void): void {
  next();
}

export function auditLog(req: unknown, res: unknown, next: () => void): void {
  next();
}
