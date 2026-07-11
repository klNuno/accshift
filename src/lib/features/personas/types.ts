/** One platform slot inside a persona: which account to switch that platform to. */
export interface PersonaAssignment {
  platformId: string;
  accountId: string;
}

/**
 * A persona is a named identity bundle. Activating it switches every assigned
 * platform to the persona's account in one action ("big brother" -> "little
 * brother"). Stored client-side (client.personas).
 *
 * `image` is an optional user-picked cover (small data URL); without it the
 * card renders a mosaic of the assigned accounts' avatars. `color` is an
 * optional card tint ("" = none), same presets as account cards.
 */
export interface Persona {
  id: string;
  name: string;
  color: string;
  image?: string | null;
  assignments: PersonaAssignment[];
}
