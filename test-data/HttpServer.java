package com.example.server;

import java.io.*;
import java.net.*;
import java.nio.file.*;
import java.time.Instant;
import java.util.*;
import java.util.concurrent.*;
import java.util.logging.Logger;

/**
 * Minimal HTTP server with routing and middleware support.
 * Demonstrates generics, lambdas, records, and sealed interfaces.
 */
public class HttpServer {

    private static final Logger LOG = Logger.getLogger(HttpServer.class.getName());

    // --- Request/Response records ---

    public record Request(String method, String path, Map<String, String> headers, byte[] body) {
        public String header(String name) {
            return headers.getOrDefault(name.toLowerCase(), "");
        }
    }

    public record Response(int status, Map<String, String> headers, byte[] body) {
        public static Response ok(String text) {
            return new Response(200, Map.of("Content-Type", "text/plain"), text.getBytes());
        }

        public static Response json(String json) {
            return new Response(200, Map.of("Content-Type", "application/json"), json.getBytes());
        }

        public static Response notFound() {
            return new Response(404, Map.of(), "Not Found".getBytes());
        }
    }

    // --- Routing ---

    public sealed interface Route permits ExactRoute, PrefixRoute {
        boolean matches(Request req);
    }

    public record ExactRoute(String method, String path) implements Route {
        @Override
        public boolean matches(Request req) {
            return req.method().equals(method) && req.path().equals(path);
        }
    }

    public record PrefixRoute(String method, String prefix) implements Route {
        @Override
        public boolean matches(Request req) {
            return req.method().equals(method) && req.path().startsWith(prefix);
        }
    }

    @FunctionalInterface
    public interface Handler {
        Response handle(Request request) throws Exception;
    }

    @FunctionalInterface
    public interface Middleware {
        Handler wrap(Handler next);
    }

    // --- Server implementation ---

    private final List<Map.Entry<Route, Handler>> routes = new ArrayList<>();
    private final List<Middleware> middlewares = new ArrayList<>();
    private final ExecutorService pool;

    public HttpServer(int threads) {
        this.pool = Executors.newFixedThreadPool(threads);
    }

    public void get(String path, Handler handler) {
        routes.add(Map.entry(new ExactRoute("GET", path), handler));
    }

    public void post(String path, Handler handler) {
        routes.add(Map.entry(new ExactRoute("POST", path), handler));
    }

    public void prefix(String method, String prefix, Handler handler) {
        routes.add(Map.entry(new PrefixRoute(method, prefix), handler));
    }

    public void use(Middleware middleware) {
        middlewares.add(middleware);
    }

    public void start(int port) throws IOException {
        try (var serverSocket = new ServerSocket(port)) {
            LOG.info("Listening on port " + port);
            while (!Thread.currentThread().isInterrupted()) {
                Socket client = serverSocket.accept();
                pool.submit(() -> handleClient(client));
            }
        }
    }

    private void handleClient(Socket client) {
        try (client) {
            var request = parseRequest(client.getInputStream());
            var handler = findHandler(request);

            // Apply middleware chain
            for (var mw : middlewares.reversed()) {
                handler = mw.wrap(handler);
            }

            var response = handler.handle(request);
            writeResponse(client.getOutputStream(), response);
        } catch (Exception e) {
            LOG.warning("Error: " + e.getMessage());
        }
    }

    private Handler findHandler(Request request) {
        return routes.stream()
                .filter(entry -> entry.getKey().matches(request))
                .map(Map.Entry::getValue)
                .findFirst()
                .orElse(req -> Response.notFound());
    }

    // --- Parsing (simplified) ---

    private Request parseRequest(InputStream in) throws IOException {
        var reader = new BufferedReader(new InputStreamReader(in));
        String line = reader.readLine();
        if (line == null) throw new IOException("Empty request");

        String[] parts = line.split(" ");
        String method = parts[0];
        String path = parts.length > 1 ? parts[1] : "/";

        Map<String, String> headers = new HashMap<>();
        while ((line = reader.readLine()) != null && !line.isEmpty()) {
            int colon = line.indexOf(':');
            if (colon > 0) {
                headers.put(line.substring(0, colon).trim().toLowerCase(),
                           line.substring(colon + 1).trim());
            }
        }

        return new Request(method, path, headers, new byte[0]);
    }

    private void writeResponse(OutputStream out, Response response) throws IOException {
        var writer = new PrintWriter(out);
        writer.printf("HTTP/1.1 %d OK\r\n", response.status());
        response.headers().forEach((k, v) -> writer.printf("%s: %s\r\n", k, v));
        writer.print("\r\n");
        writer.flush();
        out.write(response.body());
        out.flush();
    }

    // --- Main ---

    public static void main(String[] args) throws IOException {
        var server = new HttpServer(Runtime.getRuntime().availableProcessors());

        // Logging middleware
        server.use(next -> request -> {
            var start = Instant.now();
            var response = next.handle(request);
            var elapsed = java.time.Duration.between(start, Instant.now());
            LOG.info("%s %s → %d (%dms)".formatted(
                    request.method(), request.path(),
                    response.status(), elapsed.toMillis()));
            return response;
        });

        // Routes
        server.get("/", req -> Response.ok("Hello, World!"));
        server.get("/health", req -> Response.json("{\"status\":\"ok\"}"));
        server.prefix("GET", "/files/", req -> {
            var filePath = Path.of(".", req.path());
            if (Files.exists(filePath)) {
                return new Response(200,
                        Map.of("Content-Type", Files.probeContentType(filePath)),
                        Files.readAllBytes(filePath));
            }
            return Response.notFound();
        });

        server.start(8080);
    }
}
