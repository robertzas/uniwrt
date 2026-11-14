/** Reading a plan to trace (M6.2c2): its own module, so component tests can stand in for it. */
import { readPixels } from "../../coverage/footprint";
import { type TracedWall, type TraceInput, trace, workSize } from "./trace";

/** The walls traced from the plan at `src`; undefined where it can't be read. */
export async function tracePlan(
  src: string,
  input: Omit<TraceInput, "pixels">,
): Promise<TracedWall[] | undefined> {
  const size = workSize(input.width, input.height);
  const pixels = await readPixels(src, size.width, size.height);
  if (!pixels) return undefined;
  // Let the toolbar say it's tracing before the work holds the thread.
  await new Promise((resolve) => setTimeout(resolve, 0));
  return trace({ ...input, pixels });
}
