import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import { createAppRouter } from "./routes";
import { Shell } from "./Shell";

const Router = createAppRouter();

export function App() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { staleTime: 10_000 } },
  });
  return (
    <QueryClientProvider client={queryClient}>
      <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
    </QueryClientProvider>
  );
}
