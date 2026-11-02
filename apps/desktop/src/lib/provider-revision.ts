/** An existing provider can be used as a configuration starting point.
 *  Provider IDs, credentials and probe results are intentionally never copied.
 *  Native storage decides whether identical settings reuse an existing ID.
 */
export type ProviderRevisionSource = {
  id: string;
  name: string;
  kind?: string;
  endpoint: string;
  model: string;
  remembered_key?: boolean;
};

export type ProviderRevisionDraft = {
  sourceId: string;
  name: string;
  kind: string;
  endpoint: string;
  model: string;
  key: string;
  rememberKey: boolean;
  sourceRememberedKey: boolean;
};

export function prepareProviderRevision(profile: ProviderRevisionSource): ProviderRevisionDraft {
  return {
    sourceId: profile.id,
    name: profile.name,
    kind: profile.kind ?? 'openai_chat',
    endpoint: profile.endpoint,
    model: profile.model,
    key: '',
    rememberKey: false,
    sourceRememberedKey: profile.remembered_key === true,
  };
}
