package main

import (
	"context"
	"errors"
	"log"
	"os"
	"strconv"
	"strings"

	"github.com/gofiber/fiber/v2"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

type User struct {
	ID       int64  `json:"id"`
	Username string `json:"username"`
	Email    string `json:"email"`
	Role     string `json:"role"`
}

type UsersResponse struct {
	Users  []User `json:"users"`
	Source string `json:"source"`
	Count  int    `json:"count"`
}

type PlainResponse struct {
	Message string `json:"message"`
	Source  string `json:"source"`
	Values  []int  `json:"values"`
}

type UserResponse struct {
	User   User   `json:"user"`
	Source string `json:"source"`
}

type WriteResponse struct {
	ID      int64  `json:"id"`
	Source  string `json:"source"`
	Written bool   `json:"written"`
}

func parseIDs(raw string) ([]int64, error) {
	parts := strings.Split(raw, ",")
	if len(parts) < 1 || len(parts) > 100 {
		return nil, fiber.ErrBadRequest
	}
	ids := make([]int64, 0, len(parts))
	for _, part := range parts {
		id, err := strconv.ParseInt(strings.TrimSpace(part), 10, 64)
		if err != nil || id < 1 {
			return nil, fiber.ErrBadRequest
		}
		ids = append(ids, id)
	}
	return ids, nil
}

func main() {
	app := fiber.New(fiber.Config{
		DisableStartupMessage: true,
	})

	databaseURL := os.Getenv("DATABASE_URL")
	if databaseURL == "" {
		databaseURL = "postgres://ferris:ferris@127.0.0.1:5432/ferris_bench"
	}
	config, err := pgxpool.ParseConfig(databaseURL)
	if err != nil {
		log.Fatalf("invalid DATABASE_URL: %v", err)
	}
	config.MaxConns = 20
	config.MinConns = 20
	db, err := pgxpool.NewWithConfig(context.Background(), config)
	if err != nil {
		log.Fatalf("could not create PostgreSQL pool: %v", err)
	}
	if err := db.Ping(context.Background()); err != nil {
		log.Fatalf("could not connect to PostgreSQL: %v", err)
	}
	defer db.Close()

	app.Get("/health", func(c *fiber.Ctx) error {
		return c.JSON(fiber.Map{
			"status":    "ok",
			"framework": "Fiber (Go)",
			"port":      8080,
		})
	})

	app.Get("/api/v1/health", func(c *fiber.Ctx) error {
		return c.JSON(fiber.Map{
			"status":    "ok",
			"framework": "Fiber (Go)",
			"port":      8080,
		})
	})

	app.Get("/api/v1/plain", func(c *fiber.Ctx) error {
		return c.JSON(PlainResponse{
			Message: "FerrisTelemetry benchmark payload",
			Source:  "memory",
			Values:  []int{1, 2, 3, 4, 5, 6, 7, 8, 9, 10},
		})
	})

	app.Get("/api/v1/users", func(c *fiber.Ctx) error {
		rows, err := db.Query(context.Background(), "SELECT id, username, email, role FROM users ORDER BY id LIMIT 10")
		if err != nil {
			log.Printf("PostgreSQL query error: %v", err)
			return c.Status(fiber.StatusServiceUnavailable).JSON(fiber.Map{
				"error":  "postgres unavailable",
				"source": "unavailable",
			})
		}
		defer rows.Close()

		users := make([]User, 0, 10)
		for rows.Next() {
			var u User
			if err := rows.Scan(&u.ID, &u.Username, &u.Email, &u.Role); err != nil {
				return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{"error": "row scan failed"})
			}
			users = append(users, u)
		}
		if err := rows.Err(); err != nil {
			return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{"error": "row iteration failed"})
		}
		return c.JSON(UsersResponse{
			Users:  users,
			Source: "postgres",
			Count:  len(users),
		})
	})

	app.Get("/api/v1/user", func(c *fiber.Ctx) error {
		id, err := strconv.ParseInt(c.Query("id"), 10, 64)
		if err != nil || id < 1 {
			return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{"error": "id must be a positive integer"})
		}
		var u User
		err = db.QueryRow(context.Background(),
			"SELECT id, username, email, role FROM users WHERE id = $1", id,
		).Scan(&u.ID, &u.Username, &u.Email, &u.Role)
		if errors.Is(err, pgx.ErrNoRows) {
			return c.SendStatus(fiber.StatusNotFound)
		}
		if err != nil {
			return c.Status(fiber.StatusServiceUnavailable).JSON(fiber.Map{"error": "postgres query failed"})
		}
		return c.JSON(UserResponse{User: u, Source: "postgres"})
	})

	app.Get("/api/v1/queries", func(c *fiber.Ctx) error {
		ids, err := parseIDs(c.Query("ids"))
		if err != nil {
			return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{"error": "ids must contain between 1 and 100 positive integers"})
		}
		rows, err := db.Query(context.Background(),
			"SELECT id, username, email, role FROM users WHERE id = ANY($1::bigint[]) ORDER BY id", ids,
		)
		if err != nil {
			return c.Status(fiber.StatusServiceUnavailable).JSON(fiber.Map{"error": "postgres query failed"})
		}
		defer rows.Close()
		users := make([]User, 0, len(ids))
		for rows.Next() {
			var u User
			if err := rows.Scan(&u.ID, &u.Username, &u.Email, &u.Role); err != nil {
				return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{"error": "row scan failed"})
			}
			users = append(users, u)
		}
		if err := rows.Err(); err != nil {
			return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{"error": "row iteration failed"})
		}
		return c.JSON(UsersResponse{Users: users, Source: "postgres", Count: len(users)})
	})

	writeHandler := func(c *fiber.Ctx) error {
		id, err := strconv.ParseInt(c.Query("id"), 10, 64)
		if err != nil || id < 1 {
			return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{"error": "id must be a positive integer"})
		}
		var writtenID int64
		err = db.QueryRow(context.Background(),
			"INSERT INTO benchmark_writes (id, value) VALUES ($1, 'benchmark') ON CONFLICT (id) DO UPDATE SET value = EXCLUDED.value, updated_at = NOW() RETURNING id",
			id,
		).Scan(&writtenID)
		if err != nil {
			return c.Status(fiber.StatusServiceUnavailable).JSON(fiber.Map{"error": "postgres write failed"})
		}
		return c.JSON(WriteResponse{ID: writtenID, Source: "postgres", Written: true})
	}
	app.Get("/api/v1/write", writeHandler)
	app.Post("/api/v1/write", writeHandler)

	app.Get("/api/v1/compute", func(c *fiber.Ctx) error {
		var total int64
		for i := int64(0); i < 1000; i++ {
			total += i * i
		}
		return c.JSON(fiber.Map{
			"result":    total,
			"framework": "Fiber",
		})
	})

	port := os.Getenv("FIBER_PORT")
	if port == "" {
		port = "8080"
	}
	log.Printf("INFO: Fiber benchmark server running on http://127.0.0.1:%s", port)
	log.Fatal(app.Listen(":" + port))
}
