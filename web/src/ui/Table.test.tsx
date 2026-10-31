import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { createSignal } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { type Column, Table } from "./Table";

interface Node {
  id: string;
  name: string;
  clients: number;
}

const columns: Column<Node>[] = [
  { id: "name", header: "Name", cell: (n) => n.name },
  { id: "clients", header: "Clients", cell: (n) => n.clients, align: "end" },
];
const nodes = (count: number): Node[] =>
  Array.from({ length: count }, (_, i) => ({ id: `n${i}`, name: `ap-${i}`, clients: i }));

describe("Table", () => {
  it("renders a header and a row per item", () => {
    render(() => <Table label="Devices" columns={columns} rows={nodes(3)} rowKey={(n) => n.id} />);
    const table = screen.getByRole("table", { name: "Devices" });
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((h) => h.textContent),
    ).toEqual(["Name", "Clients"]);
    expect(within(table).getAllByRole("row")).toHaveLength(4);
    expect(within(table).getAllByRole("columnheader")[1]).toHaveClass("text-right");
  });

  it("shows the empty state without rows", () => {
    render(() => (
      <Table label="Devices" columns={columns} rows={[]} rowKey={(n) => n.id} empty="No devices" />
    ));
    expect(screen.getByText("No devices")).toBeInTheDocument();
  });

  it("activates rows by click and by keyboard", async () => {
    const user = userEvent.setup();
    const onRowClick = vi.fn();
    render(() => (
      <Table
        label="Devices"
        columns={columns}
        rows={nodes(2)}
        rowKey={(n) => n.id}
        onRowClick={onRowClick}
      />
    ));
    const [, first, second] = screen.getAllByRole("row");
    await user.click(first as HTMLElement);
    expect(onRowClick).toHaveBeenLastCalledWith(expect.objectContaining({ id: "n0" }));
    (second as HTMLElement).focus();
    await user.keyboard("{Enter}");
    expect(onRowClick).toHaveBeenLastCalledWith(expect.objectContaining({ id: "n1" }));
  });

  it("keeps rows in place when the data updates", () => {
    const [rows, setRows] = createSignal(nodes(2));
    render(() => <Table label="Devices" columns={columns} rows={rows()} rowKey={(n) => n.id} />);
    const before = screen.getAllByRole("row")[1];
    setRows((r) => [...r, { id: "n9", name: "ap-9", clients: 9 }]);
    return Promise.resolve().then(() => {
      expect(screen.getAllByRole("row")).toHaveLength(4);
      expect(screen.getAllByRole("row")[1]).toBe(before);
    });
  });

  describe("virtualized", () => {
    // jsdom has no layout: give the scroll container a 400 px viewport.
    beforeEach(() => {
      vi.spyOn(Element.prototype, "clientHeight", "get").mockReturnValue(400);
      vi.spyOn(Element.prototype, "clientWidth", "get").mockReturnValue(800);
    });
    afterEach(() => vi.restoreAllMocks());

    it("renders only the rows in view above the threshold", async () => {
      render(() => (
        <Table
          label="Clients"
          columns={columns}
          rows={nodes(1000)}
          rowKey={(n) => n.id}
          virtualizeAbove={200}
        />
      ));
      await waitFor(() => expect(screen.getAllByRole("row").length).toBeGreaterThan(1));
      const rendered = screen.getAllByRole("row").length;
      expect(rendered).toBeLessThan(100);
      expect(screen.getByText("ap-0")).toBeInTheDocument();
      expect(screen.queryByText("ap-999")).not.toBeInTheDocument();
    });
  });
});
