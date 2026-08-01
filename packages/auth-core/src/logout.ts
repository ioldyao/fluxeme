export type LogoutHandler<TResult> = () => Promise<TResult>

export async function logout<TResult>(handler: LogoutHandler<TResult>): Promise<TResult> {
  return handler()
}
