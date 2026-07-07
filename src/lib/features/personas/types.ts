/** One platform slot inside a persona: which account to switch that platform to. */
export interface PersonaAssignment {
  platformId: string;
  accountId: string;
}

/**
 * A persona is a named identity bundle. Activating it switches every assigned
 * platform to the persona's account in one action ("big brother" -> "little
 * brother"). Stored client-side (client.personas).
 */
export interface Persona {
  id: string;
  name: string;
  emoji: string;
  color: string;
  assignments: PersonaAssignment[];
}
