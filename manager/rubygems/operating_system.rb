# Shadows Debian's copy, which comes later on the load path, so non-root users
# install gems under their own HOME instead of the root-owned /var/lib/gems.
load File.join(RbConfig::CONFIG['vendordir'], 'rubygems/defaults/operating_system.rb')

unless Process.uid.zero?
  class << Gem
    alias_method :system_default_dir, :default_dir
    alias_method :system_default_path, :default_path

    def user_dir
      File.join(user_home, '.gem', ruby_engine, RbConfig::CONFIG['ruby_version'])
    end

    def default_dir
      user_dir
    end

    def default_path
      system_default_path | [system_default_dir]
    end

    def default_bindir
      File.join(user_home, '.gem', 'bin')
    end
  end
end
