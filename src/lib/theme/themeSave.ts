/** Writes a theme draft. A rejection keeps the open editor and its fields. */
export async function persistThemeDraft(save: () => Promise<unknown>): Promise<"saved" | "failed"> {
  try {
    await save();
    return "saved";
  } catch (error) {
    console.error("theme save failed", error);
    return "failed";
  }
}
