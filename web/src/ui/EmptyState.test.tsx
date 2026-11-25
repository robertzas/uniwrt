import { render, screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { Inbox } from "lucide";
import { describe, expect, it, vi } from "vitest";
import { EmptyState } from "./EmptyState";

describe("EmptyState", () => {
  it("says what is missing and offers its next steps, the first as the primary one", async () => {
    const add = vi.fn();
    render(() => (
      <EmptyState
        icon={Inbox}
        title="Nothing yet"
        description="Add one."
        actions={[
          { label: "Adopt", href: "/devices?adopt=1" },
          { label: "Add", onClick: add },
        ]}
      />
    ));
    expect(screen.getByRole("heading", { name: "Nothing yet" })).toBeInTheDocument();
    const adopt = screen.getByRole("link", { name: "Adopt" });
    expect(adopt).toHaveAttribute("href", "/devices?adopt=1");
    expect(adopt).toHaveClass("bg-accent-fill");
    const button = screen.getByRole("button", { name: "Add" });
    expect(button).not.toHaveClass("bg-accent-fill");
    await userEvent.click(button);
    expect(add).toHaveBeenCalledOnce();
  });

  it("offers nothing without actions", () => {
    render(() => <EmptyState icon={Inbox} title="Nothing yet" description="Wait." actions={[]} />);
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});
