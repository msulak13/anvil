// Pure-TypeScript interface, aliased via @Binds inside AppModule.
export interface Logger {
  info(message: string): void;
  warn(message: string): void;
}
