import { Router } from "@solidjs/router";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import { routes } from "./routes";
import { Shell } from "./Shell";

export function App() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { staleTime: 10_000 } },
  });
  return (
    <QueryClientProvider client={queryClient}>
      <Router root={Shell}>{routes}</Router>
    </QueryClientProvider>
  );
}
