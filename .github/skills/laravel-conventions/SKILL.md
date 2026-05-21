---
name: laravel-conventions
description: Modern Laravel 11+ / PHP 8.3+ coding standards reference. Use when writing or reviewing PHP/Laravel code to ensure convention compliance.
model: gpt-5.3-codex
---

# Laravel Coding Conventions (Laravel 11+ / PHP 8.3+)

Reference skill for modern Laravel coding standards and PHP 8.3+ best practices.

## Architecture Pattern

```
Controller -> FormRequest -> Action/Service -> Model
```

- **Controllers**: Thin, single-responsibility. Invoke actions or services, return API resources
- **Actions**: Single-purpose classes for discrete operations (e.g., `CreateUser`, `ProcessPayment`)
- **Services**: Coordinate multiple actions or complex business logic
- **Models**: Eloquent models with relationships, scopes, casts, and accessors/mutators
- **DTOs**: Readonly data transfer objects for passing structured data between layers

## PHP 8.3+ Features to Use

```php
declare(strict_types=1);

// Readonly classes for DTOs
readonly class CreateUserData {
    public function __construct(
        public string $name,
        public string $email,
        public ?string $phone = null,
    ) {}
}

// Enums instead of constants
enum UserStatus: string {
    case Active = 'active';
    case Suspended = 'suspended';
    case Pending = 'pending';
}

// First-class callable syntax
$users->map($this->transformUser(...));

// Named arguments for clarity
Cache::put(key: $cacheKey, value: $data, ttl: 3600);

// Match expressions over switch
$label = match($status) {
    UserStatus::Active => 'Active User',
    UserStatus::Suspended => 'Account Suspended',
    default => 'Unknown',
};
```

## Naming Conventions

| Type | Convention | Example |
|------|------------|---------|
| Classes | PascalCase | `UserService` |
| Methods | camelCase | `getUserById` |
| Properties/Variables | camelCase | `$userData` |
| Constants | UPPER_SNAKE_CASE | `MAX_RETRIES` |
| Database tables | plural, snake_case | `user_accounts` |
| Database columns | snake_case | `created_at` |
| Routes | kebab-case | `/user-profiles` |
| Config keys | snake_case | `cache.default_ttl` |
| Enums | PascalCase | `UserStatus::Active` |

## Code Style

- PSR-12 compliance, enforced via Laravel Pint
- `declare(strict_types=1)` at the top of every PHP file
- Single quotes for strings (unless interpolation needed)
- Short array syntax `[]` with trailing commas
- Type declarations on all parameters, return types, and properties
- No `mixed` type unless absolutely necessary
- Constructor property promotion where appropriate

## Directory Structure (Laravel 11+)

```
app/
├── Actions/              # Single-purpose action classes
├── Console/Commands/     # Artisan commands
├── DTOs/                 # Readonly data transfer objects
├── Enums/                # PHP 8.1+ backed enums
├── Events/               # Event classes
├── Exceptions/           # Custom exceptions
├── Http/
│   ├── Controllers/      # API/Web controllers
│   ├── Middleware/        # Request middleware
│   ├── Requests/         # FormRequest validation
│   └── Resources/        # API Resources (JSON transformations)
├── Jobs/                 # Queued jobs
├── Listeners/            # Event listeners
├── Mail/                 # Mailable classes
├── Models/               # Eloquent models
│   └── Concerns/         # Model traits
├── Notifications/        # Notification classes
├── Observers/            # Model observers
├── Policies/             # Authorization policies
├── Providers/            # Service providers
├── Rules/                # Custom validation rules
└── Services/             # Business logic services
database/
├── factories/            # Model factories
├── migrations/           # Database migrations
└── seeders/              # Database seeders
routes/
├── api.php               # API routes
├── web.php               # Web routes
└── console.php           # Console routes (Laravel 11+)
tests/
├── Feature/              # Feature/integration tests
├── Unit/                 # Unit tests
└── Pest.php              # Pest configuration
```

## Eloquent Best Practices

```php
// Use $casts property (not method unless dynamic)
protected $casts = [
    'email_verified_at' => 'datetime',
    'status' => UserStatus::class,
    'settings' => 'array',
    'is_admin' => 'boolean',
];

// Scope queries for reuse
public function scopeActive(Builder $query): Builder
{
    return $query->where('status', UserStatus::Active);
}

// Prevent lazy loading in development
// In AppServiceProvider::boot()
Model::preventLazyLoading(! app()->isProduction());

// Use strict mode
Model::shouldBeStrict(! app()->isProduction());

// Eager load relationships
User::with(['posts', 'profile'])->get();

// Use whenLoaded in API Resources
'posts' => PostResource::collection($this->whenLoaded('posts')),
```

## Database Migrations

### Naming Convention

```
YYYY_MM_DD_HHMMSS_description.php
```

### Best Practices

- One concern per migration file
- Always include `down()` method for rollbacks
- Never modify migrations that have been run in production
- Use `foreignId()` and `constrained()` for foreign keys
- Use `after()` to control column order

```php
Schema::create('posts', function (Blueprint $table) {
    $table->id();
    $table->foreignId('user_id')->constrained()->cascadeOnDelete();
    $table->string('title');
    $table->string('slug')->unique();
    $table->text('body');
    $table->enum('status', ['draft', 'published', 'archived'])->default('draft');
    $table->timestamp('published_at')->nullable();
    $table->timestamps();
    $table->softDeletes();

    $table->index(['status', 'published_at']);
});
```

## API Resources

```php
// Always use API Resources for JSON responses
class UserResource extends JsonResource
{
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'name' => $this->name,
            'email' => $this->email,
            'posts' => PostResource::collection($this->whenLoaded('posts')),
            'created_at' => $this->created_at->toISOString(),
        ];
    }
}

// In controller
return UserResource::make($user);
return UserResource::collection($users);
```

## Queues and Jobs

```php
// Implement ShouldQueue for async processing
class ProcessPayment implements ShouldQueue
{
    use Queueable;

    public int $tries = 3;
    public int $backoff = 60;
    public int $timeout = 120;

    public function __construct(
        private readonly Order $order,
    ) {}

    public function handle(PaymentGateway $gateway): void
    {
        $gateway->charge($this->order);
    }

    public function failed(\Throwable $exception): void
    {
        // Notify team of failure
    }
}
```

## Events and Listeners

```php
// Use typed events
class OrderPlaced
{
    public function __construct(
        public readonly Order $order,
    ) {}
}

// Auto-discovered listeners (Laravel 11+)
// Just type-hint the event in handle()
class SendOrderConfirmation
{
    public function handle(OrderPlaced $event): void
    {
        $event->order->user->notify(new OrderConfirmationNotification($event->order));
    }
}
```

## Routes

```php
// Use controller class references
Route::apiResource('users', UserController::class);

// Group related routes
Route::prefix('admin')->middleware('auth:sanctum', 'admin')->group(function () {
    Route::apiResource('users', Admin\UserController::class);
});

// Single-action controllers
Route::post('/webhooks/stripe', StripeWebhookController::class);
```

## Security

- Use Laravel Sanctum or Passport for API authentication
- Always validate with FormRequest classes
- Use Policies for authorization (not Gates in controllers)
- Never expose internal IDs if security-sensitive -- use UUIDs or hashids
- Use `$request->validated()` to only access validated data
- Set rate limiting on API routes
- Use `encrypt()`/`decrypt()` for sensitive data at rest

## Error Handling

```php
// Custom exceptions with render method
class InsufficientFundsException extends Exception
{
    public function render(Request $request): JsonResponse
    {
        return response()->json([
            'message' => 'Insufficient funds for this transaction.',
        ], 422);
    }
}

// Use abort helpers
abort_if(! $user->canAccess($resource), 403);
abort_unless($order->isPending(), 422, 'Order is no longer pending.');
```

## Testing with Pest

```php
// Feature test
it('creates a user', function () {
    $response = postJson('/api/users', [
        'name' => 'Jane Doe',
        'email' => 'jane@example.com',
        'password' => 'secure-password',
    ]);

    $response->assertCreated()
        ->assertJsonPath('data.name', 'Jane Doe');

    $this->assertDatabaseHas('users', ['email' => 'jane@example.com']);
});

// Unit test with mocking
it('calculates order total with tax', function () {
    $order = Order::factory()->create(['subtotal' => 10000]);

    expect($order->totalWithTax())->toBe(10800);
});

// Use datasets for parameterized tests
it('validates required fields', function (string $field) {
    $data = User::factory()->make()->toArray();
    unset($data[$field]);

    postJson('/api/users', $data)->assertUnprocessable()
        ->assertJsonValidationErrors($field);
})->with(['name', 'email', 'password']);
```

### Running Tests

```bash
# All tests
php artisan test

# With Pest directly
./vendor/bin/pest

# Specific test file
./vendor/bin/pest tests/Feature/UserTest.php

# Filter by name
./vendor/bin/pest --filter "creates a user"

# Parallel execution
php artisan test --parallel
```

### Running Linter

```bash
./vendor/bin/pint

# Check only (no changes)
./vendor/bin/pint --test

# Static analysis
./vendor/bin/phpstan analyse
```
