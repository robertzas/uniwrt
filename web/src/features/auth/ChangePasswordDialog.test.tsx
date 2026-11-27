import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ChangePasswordDialog } from "./ChangePasswordDialog";

let calls: { url: string; body: unknown }[] = [];
let answer: () => Response;

beforeEach(() => {
  calls = [];
  answer = () => new Response(null, { status: 204 });
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url, body: init?.body ? JSON.parse(String(init.body)) : undefined });
      return answer();
    }),
  );
});

async function fill(current: string, next: string, again: string) {
  const user = userEvent.setup();
  await user.type(screen.getByLabelText("Current password"), current);
  await user.type(screen.getByLabelText("New password"), next);
  await user.type(screen.getByLabelText("New password again"), again);
  await user.click(screen.getByRole("button", { name: "Change password" }));
}

describe("Change your password", () => {
  it("checks the new password before sending it", async () => {
    render(() => <ChangePasswordDialog onClose={() => {}} />);
    await fill("the old password", "short", "short");
    expect(screen.getByText("Use at least 12 characters.")).toBeInTheDocument();
    await userEvent.setup().type(screen.getByLabelText("New password"), " but long now");
    await userEvent.setup().click(screen.getByRole("button", { name: "Change password" }));
    expect(screen.getByText("The passwords don’t match.")).toBeInTheDocument();
    expect(calls).toEqual([]);
  });

  it("sends the current and new passwords, then closes", async () => {
    const onClose = vi.fn();
    render(() => <ChangePasswordDialog onClose={onClose} />);
    await fill("the old password", "a brand new password", "a brand new password");
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(calls).toEqual([
      {
        url: "/api/v1/auth/password",
        body: { current: "the old password", new: "a brand new password" },
      },
    ]);
  });

  it("shows why the server refused", async () => {
    answer = () =>
      Response.json(
        { title: "Bad Request", detail: "The current password is wrong" },
        { status: 400, headers: { "content-type": "application/problem+json" } },
      );
    const onClose = vi.fn();
    render(() => <ChangePasswordDialog onClose={onClose} />);
    await fill("not my password", "a brand new password", "a brand new password");
    expect(await screen.findByRole("alert")).toHaveTextContent("The current password is wrong");
    expect(onClose).not.toHaveBeenCalled();
  });
});
