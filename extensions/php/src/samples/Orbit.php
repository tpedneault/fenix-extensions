<?php

declare(strict_types=1);

namespace Orbit\Desk;

use DateTimeImmutable;
use InvalidArgumentException;

interface Tracker
{
    public function track(Satellite $satellite): void;
}

enum Status: string
{
    case Nominal = 'nominal';
    case Safe = 'safe';

    public function label(): string
    {
        return ucfirst($this->value);
    }
}

final readonly class Satellite
{
    public const SECONDS = 60;

    public function __construct(
        public string $name,
        public float $period = 1.5e3,
        private ?DateTimeImmutable $launched = null,
        public Status $status = Status::Nominal,
    ) {
        if ($name === '') {
            throw new InvalidArgumentException("A satellite has a name\n");
        }
    }

    #[\Override]
    public function describe(): string
    {
        return sprintf('%s: %d min', $this->name, (int) ($this->period / self::SECONDS));
    }

    public function isLow(): bool
    {
        return match (true) {
            $this->period < 90 * self::SECONDS => true,
            default => false,
        };
    }
}

/** @param list<Satellite> $satellites */
function brightest(array $satellites): ?Satellite
{
    $best = null;
    foreach ($satellites as $satellite) {
        if ($best === null || $satellite->period < $best->period) {
            $best = $satellite;
        }
    }
    return $best;
}

// Two at once.
$pair = [new Satellite('ISS', 5520.0), new Satellite('Hubble')];
$names = array_map(fn (Satellite $s): string => $s->describe(), $pair);
echo implode("\n", $names), PHP_EOL;
?>
<p>Tracked: <?= count($pair) ?></p>
