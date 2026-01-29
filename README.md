# Repo Name
### Mission:
What does this thing do? What problem is being solved?

### Resource Profile:
What resources are most critical to this code? (cpu, mem, storage)

### Internal / External Dependencies: 
Things like redis, bigquery, bucket, etc?

### Testing Procedure
How do we test the code locally or in devops process?

### Notes
Are there any nuances to be shared? 

# User Flow
## Init
1. OAUTH -> JWT
2. Create a new org.
3. Create a new installation key, maybe option for user to specify tags.
4. Create a new Webhook Adapter, specify sensor_seed_key_path = event/mtd/install_id
  4.1 this implies generating a random secret for the hook, record that locally along with the adapter name and OID. https://docs.limacharlie.io/docs/tutorial-creating-a-webhook-adapter?highlight=Using%20the%20webhook%20adapter
  4.2 store this somewhere???
5. print the URL to the webapp org for the user and give them a OID:HOOK_NAME:SECRET value (this is what we might map eventually) so that they or others can onboard.


## Onboard
1. User provides the OID:HOOK_NAME:SECRET.
2. Register the hooks etc.
3. Profit.
