# frozen_string_literal: true

class PromotionTarget
  def self.version_code(from_track:, code_fetcher:, explicit: nil)
    override = explicit.to_s.strip
    unless override.empty?
      code = Integer(override, exception: false)
      raise ArgumentError, "Version code #{explicit.inspect} is not a positive integer" unless code&.positive?

      return code
    end

    Array(code_fetcher.call(from_track)).map(&:to_i).select(&:positive?).max
  end
end
