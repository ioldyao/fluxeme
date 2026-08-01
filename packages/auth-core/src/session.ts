export type SessionFetcher<TSession> = () => Promise<TSession>

export async function fetchSession<TSession>(fetcher: SessionFetcher<TSession>): Promise<TSession> {
  return fetcher()
}
