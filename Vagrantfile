# -*- mode: ruby -*-
# vi: set ft=ruby :
#
# LACS E2E Test VM
# ================
#
# LIMITATION: This VM uses fedora/42-cloud-base, NOT Fedora Silverblue.
# Official Silverblue Vagrant boxes are scarce/outdated. This base image
# has dnf and systemd but is NOT an rpm-ostree system. Consequences:
#
#   - Stories 1-7 (read-only) work: the daemon, brain, and state collection
#     commands (df, free, ps, systemctl, podman, etc.) all function normally.
#
#   - Stories 8-10 (destructive) that rely on rpm-ostree mutation
#     (UpdateSystem, RebaseSystem, InstallPackages) CANNOT be tested here.
#     They need a real Silverblue install.
#
# For destructive rpm-ostree testing on Silverblue:
#   - Download the Silverblue ISO from https://fedoraproject.org/silverblue/
#   - Use quickemu or install manually in a VM
#   - Or build a custom Silverblue Vagrant box with Packer and use an
#     optional Vagrantfile.silverblue (not included in this repo)
#
# Usage:
#   vagrant up                          # libvirt by default on Linux
#   VAGRANT_PROVIDER=virtualbox vagrant up  # cross-platform fallback
#   vagrant ssh -c 'cd /vagrant && sudo tests/e2e/run-stories.sh'

Vagrant.configure("2") do |config|
  config.vm.box = "fedora/42-cloud-base"
  config.vm.hostname = "lacs-e2e"

  # --- Port forwards ---
  # Ollama API (LLM inference)
  config.vm.network "forwarded_port", guest: 11434, host: 11434, auto_correct: true
  # Optional LACS shell web preview
  config.vm.network "forwarded_port", guest: 8080, host: 8080, auto_correct: true

  # --- Synced folder ---
  config.vm.synced_folder ".", "/vagrant", type: "rsync",
    rsync__exclude: [".git/", "target/", "node_modules/", ".vagrant/"]

  # --- Resources ---
  # 4 GB RAM is the minimum for Ollama with a 1B model + Rust compilation.
  # 2 vCPUs keeps compile times tolerable.

  # --- Provider: libvirt (Linux default) ---
  config.vm.provider "libvirt" do |lv|
    lv.driver = "qemu"
    lv.memory = 4096
    lv.cpus = 2
    lv.machine_virtual_size = 20  # 20 GB disk
    lv.graphics_type = "none"
  end

  # --- Provider: VirtualBox (cross-platform fallback) ---
  config.vm.provider "virtualbox" do |vb|
    vb.gui = false
    vb.memory = 4096
    vb.cpus = 2
    vb.name = "lacs-e2e"
    # Resize disk to 20 GB (VirtualBox >= 6.x)
    # Note: requires vagrant-disksize plugin or manual resize
  end

  # --- Provisioner ---
  config.vm.provision "shell", path: "tests/e2e/provision.sh", privileged: true
end
