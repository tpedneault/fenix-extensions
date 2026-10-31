# frozen_string_literal: true

require "json"
require_relative "orbit/version"

# Orbits, kept by name.
module Orbit
  MU = 398_600.4418
  Period = Data.define(:seconds)

  class Satellite < Struct.new(:name, :altitude, keyword_init: true)
    attr_reader :tags

    def initialize(name:, altitude: 400, **rest)
      super(name: name, altitude: altitude)
      @tags = rest.fetch(:tags) { [] }
      @@count ||= 0
    end

    def period
      radius = 6371.0 + altitude
      Period.new(seconds: 2 * Math::PI * Math.sqrt(radius**3 / MU))
    end

    def full_name = "#{name} (#{altitude} km)"

    def classify
      case altitude
      when 0...2000 then :leo
      when 2000...35_786
        :meo
      else
        :geo
      end
    end

    def self.parse(text)
      JSON.parse(text, symbolize_names: true) => {name:, altitude:}
      new(name: name, altitude: altitude)
    rescue JSON::ParserError => e
      warn e.message
      nil
    end
  end

  def self.each_visible(satellites)
    return enum_for(__method__, satellites) unless block_given?

    satellites.each do |s|
      next if s.altitude.nil?

      yield s
    end
  end
end

if $PROGRAM_NAME == __FILE__
  sats = [Orbit::Satellite.new(name: "ISS", altitude: 420)]
  Orbit.each_visible(sats) { |s| puts s.full_name }
  1.upto(3) { |i| printf("%d\n", i) }
  x = [1, 2.5, nil, true]
  puts x.inspect, %w[a b], /\d+/.source
end
